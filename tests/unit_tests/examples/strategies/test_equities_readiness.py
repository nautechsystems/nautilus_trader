from __future__ import annotations

from datetime import datetime
from datetime import timezone
from pathlib import Path

from flux.common.account_scopes import AccountScopeConfig
from flux.common.strategy_contracts import StrategyContractEntry


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _strategy_contracts() -> tuple[StrategyContractEntry, ...]:
    return (
        StrategyContractEntry(
            strategy_id="aapl_tradexyz_makerv3",
            portfolio_asset_id="AAPL",
            maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            reference_instrument_id="AAPL.NASDAQ",
            execution_account_scope_id="hyperliquid.xyz.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
        StrategyContractEntry(
            strategy_id="msft_tradexyz_makerv3",
            portfolio_asset_id="MSFT",
            maker_instrument_id="xyz:MSFT-USD-PERP.HYPERLIQUID",
            reference_instrument_id="MSFT.NASDAQ",
            execution_account_scope_id="hyperliquid.xyz.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
    )


def _account_scopes() -> tuple[AccountScopeConfig, ...]:
    return (
        AccountScopeConfig(
            scope_id="hyperliquid.xyz.main",
            provider="hyperliquid",
            venue="HYPERLIQUID",
        ),
        AccountScopeConfig(
            scope_id="ibkr.reference.main",
            provider="ibkr",
            venue="IBKR",
        ),
        AccountScopeConfig(
            scope_id="ibkr.hedge.main",
            provider="ibkr",
            venue="IBKR",
        ),
    )


def _healthy_signal_payload() -> dict[str, object]:
    return {
        "server_ts_ms": 1_700_000_000_500,
        "strategies": [
            {
                "id": "aapl_tradexyz_makerv3",
                "params": {"max_age_ms": "10000"},
                "maker_role_map": {
                    "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
                    "ref_leg": "ibkr:AAPL.NASDAQ",
                },
                "state": {
                    "state": "bot_off",
                    "maker_role_map": {
                        "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
                        "ref_leg": "nasdaq:AAPL.NASDAQ",
                    },
                },
                "legs": {
                    "ibkr:AAPL.NASDAQ": {"age_ms": 50},
                    "hyperliquid:xyz:AAPL-USD-PERP.HYPERLIQUID": {"age_ms": 25},
                },
                "debug": {
                    "md_health": {
                        "stale_legs": [],
                        "state_stale": False,
                    },
                },
            },
            {
                "id": "msft_tradexyz_makerv3",
                "params": {"max_age_ms": "10000"},
                "maker_role_map": {
                    "maker_leg": "hyperliquid:XYZ:MSFT-USD-PERP.HYPERLIQUID",
                    "ref_leg": "ibkr:MSFT.NASDAQ",
                },
                "state": {
                    "state": "bot_off",
                    "maker_role_map": {
                        "maker_leg": "hyperliquid:XYZ:MSFT-USD-PERP.HYPERLIQUID",
                        "ref_leg": "nasdaq:MSFT.NASDAQ",
                    },
                },
                "legs": {
                    "ibkr:MSFT.NASDAQ": {"age_ms": 60},
                    "hyperliquid:xyz:MSFT-USD-PERP.HYPERLIQUID": {"age_ms": 30},
                },
                "debug": {
                    "md_health": {
                        "stale_legs": [],
                        "state_stale": False,
                    },
                },
            },
        ],
    }


def _healthy_projection_payloads() -> dict[str, dict[str, object]]:
    return {
        "ibkr.reference.main": {
            "account_scope_ids": ["ibkr.reference.main"],
            "rows": [{"exchange": "ibkr", "asset": "USD", "total": "1000"}],
            "server_ts_ms": 1_700_000_000_000,
        },
        "ibkr.hedge.main": {
            "account_scope_ids": ["ibkr.hedge.main"],
            "rows": [{"exchange": "ibkr", "asset": "USD", "total": "900"}],
            "server_ts_ms": 1_700_000_000_100,
        },
    }


def _healthy_component_payloads() -> dict[str, dict[str, object]]:
    return {
        "aapl_tradexyz_makerv3": {
            "strategy_id": "aapl_tradexyz_makerv3",
            "portfolio_id": "equities",
            "base_currency": "AAPL",
            "local_qty_base": "10",
            "ts_ms": 1_700_000_000_000,
        },
        "msft_tradexyz_makerv3": {
            "strategy_id": "msft_tradexyz_makerv3",
            "portfolio_id": "equities",
            "base_currency": "MSFT",
            "local_qty_base": "5",
            "ts_ms": 1_700_000_000_000,
        },
    }


def _utc_ms(year: int, month: int, day: int, hour: int, minute: int) -> int:
    return int(datetime(year, month, day, hour, minute, tzinfo=timezone.utc).timestamp() * 1000)


def test_evaluate_equities_readiness_passes_when_contract_surfaces_are_healthy() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=_healthy_signal_payload(),
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is True
    assert result.summary["expected_projection_scope_ids"] == [
        "ibkr.hedge.main",
        "ibkr.reference.main",
    ]
    assert result.summary["healthy_strategy_count"] == 2
    assert result.checks["balances"].ok is True
    assert result.checks["component_keys"].ok is True
    assert result.checks["profile_account_projections"].ok is True
    assert result.checks["signals"].ok is True
    assert result.checks["ibkr_auth"].ok is True


def test_evaluate_equities_readiness_fails_closed_for_live_blockers() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": True,
            "missing_required": ["msft_tradexyz_makerv3"],
        },
        signals_payload={
            "server_ts_ms": 1_700_000_000_500,
            "strategies": [
                {
                    "id": "aapl_tradexyz_makerv3",
                    "params": {"max_age_ms": "10000"},
                    "state": {"state": "bot_off"},
                    "legs": {
                        "ibkr:AAPL.NASDAQ": {"age_ms": 50},
                        "hyperliquid:xyz:AAPL-USD-PERP.HYPERLIQUID": {"age_ms": 25},
                    },
                    "debug": {"md_health": {"stale_legs": [], "state_stale": False}},
                },
                {
                    "id": "msft_tradexyz_makerv3",
                    "state": {"state": "blocked_reference_md"},
                    "legs": {},
                    "debug": {
                        "md_health": {
                            "stale_legs": ["ibkr:MSFT.NASDAQ"],
                            "state_stale": False,
                        },
                    },
                },
            ],
        },
        projection_payloads_by_scope_id={
            "ibkr.reference.main": {
                "account_scope_ids": ["ibkr.reference.main"],
                "rows": [{"exchange": "ibkr", "asset": "USD", "total": "1000"}],
                "server_ts_ms": 1_699_999_000_000,
            },
        },
        component_payloads_by_strategy_id={
            "aapl_tradexyz_makerv3": _healthy_component_payloads()["aapl_tradexyz_makerv3"],
            "msft_tradexyz_makerv3": None,
        },
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is False
    assert result.checks["balances"].details["missing_required"] == ["msft_tradexyz_makerv3"]
    assert result.checks["profile_account_projections"].details["missing_scope_ids"] == [
        "ibkr.hedge.main",
    ]
    assert result.checks["profile_account_projections"].details["stale_scope_ids"] == [
        "ibkr.reference.main",
    ]
    assert result.checks["component_keys"].details["missing_strategy_ids"] == [
        "msft_tradexyz_makerv3",
    ]
    assert result.checks["signals"].details["stale_signal_legs"] == ["ibkr:MSFT.NASDAQ"]
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "msft_tradexyz_makerv3",
    ]
    assert result.checks["ibkr_auth"].details["unhealthy_strategy_ids"] == [
        "msft_tradexyz_makerv3",
    ]


def test_evaluate_equities_readiness_thresholds_are_overridable() -> None:
    from flux.runners.equities.readiness import EquitiesReadinessThresholds
    from flux.runners.equities.readiness import evaluate_equities_readiness

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload={
            "server_ts_ms": 1_700_000_000_500,
            "strategies": [
                {
                    "id": "aapl_tradexyz_makerv3",
                    "params": {"max_age_ms": "10000"},
                    "state": {"state": "bot_off"},
                    "legs": {
                        "ibkr:AAPL.NASDAQ": {"age_ms": 50},
                        "hyperliquid:xyz:AAPL-USD-PERP.HYPERLIQUID": {"age_ms": 25},
                    },
                    "debug": {"md_health": {"stale_legs": [], "state_stale": False}},
                },
                {
                    "id": "msft_tradexyz_makerv3",
                    "params": {"max_age_ms": "10000"},
                    "state": {"state": "bot_off"},
                    "legs": {
                        "ibkr:MSFT.NASDAQ": {"age_ms": 60},
                        "hyperliquid:XYZ:MSFT-USD-PERP.HYPERLIQUID": {"age_ms": 30},
                    },
                    "debug": {
                        "md_health": {
                            "stale_legs": ["hyperliquid:XYZ:MSFT-USD-PERP.HYPERLIQUID"],
                            "state_stale": False,
                        },
                    },
                },
            ],
        },
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        now_ms_value=1_700_000_000_500,
        thresholds=EquitiesReadinessThresholds(
            max_stale_signal_legs=1,
            max_unhealthy_strategies=1,
        ),
    )

    assert result.ok is True
    assert result.checks["signals"].details["stale_signal_leg_count"] == 1
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "msft_tradexyz_makerv3",
    ]


def test_evaluate_equities_readiness_fails_when_referenced_ibkr_scopes_are_missing_from_config() -> (
    None
):
    from flux.runners.equities.readiness import evaluate_equities_readiness

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=(),
        required_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=_healthy_signal_payload(),
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is False
    assert result.summary["expected_projection_scope_ids"] == [
        "ibkr.hedge.main",
        "ibkr.reference.main",
    ]
    assert result.checks["profile_account_projections"].details["missing_config_scope_ids"] == [
        "ibkr.hedge.main",
        "ibkr.reference.main",
    ]
    assert result.checks["ibkr_auth"].ok is False


def test_evaluate_equities_readiness_fails_for_over_age_reference_legs() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["legs"]["ibkr:AAPL.NASDAQ"]["age_ms"] = 999_999
    now_ms = _utc_ms(2026, 3, 13, 9, 22)
    projection_payloads = _healthy_projection_payloads()
    for payload in projection_payloads.values():
        payload["server_ts_ms"] = now_ms - 500

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=signals_payload,
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is False
    assert result.checks["signals"].details["over_age_signal_legs"] == ["ibkr:AAPL.NASDAQ"]
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "aapl_tradexyz_makerv3",
    ]
    assert result.checks["ibkr_auth"].details["unhealthy_strategy_ids"] == [
        "aapl_tradexyz_makerv3",
    ]


def test_evaluate_equities_readiness_ignores_reference_age_outside_regular_session_when_enabled() -> (
    None
):
    from flux.runners.equities.readiness import EquitiesReadinessThresholds
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["legs"]["ibkr:AAPL.NASDAQ"]["age_ms"] = 999_999
    now_ms = _utc_ms(2026, 3, 13, 9, 22)
    projection_payloads = _healthy_projection_payloads()
    for payload in projection_payloads.values():
        payload["server_ts_ms"] = now_ms - 500

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=signals_payload,
        projection_payloads_by_scope_id=projection_payloads,
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        now_ms_value=now_ms,
        thresholds=EquitiesReadinessThresholds(
            ignore_reference_freshness_outside_regular_session=True,
        ),
    )

    assert result.ok is True
    assert result.checks["signals"].details["reference_freshness_enforced"] is False
    assert result.checks["signals"].details["regular_session_active"] is False
    assert result.checks["signals"].details["over_age_signal_legs"] == []
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == []
    assert result.checks["ibkr_auth"].ok is True


def test_evaluate_equities_readiness_still_enforces_reference_age_during_regular_session() -> None:
    from flux.runners.equities.readiness import EquitiesReadinessThresholds
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["legs"]["ibkr:AAPL.NASDAQ"]["age_ms"] = 999_999
    now_ms = _utc_ms(2026, 3, 13, 14, 0)
    projection_payloads = _healthy_projection_payloads()
    for payload in projection_payloads.values():
        payload["server_ts_ms"] = now_ms - 500

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=signals_payload,
        projection_payloads_by_scope_id=projection_payloads,
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        now_ms_value=now_ms,
        thresholds=EquitiesReadinessThresholds(
            ignore_reference_freshness_outside_regular_session=True,
        ),
    )

    assert result.ok is False
    assert result.checks["signals"].details["reference_freshness_enforced"] is True
    assert result.checks["signals"].details["regular_session_active"] is True
    assert result.checks["signals"].details["over_age_signal_legs"] == ["ibkr:AAPL.NASDAQ"]
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "aapl_tradexyz_makerv3",
    ]


def test_evaluate_equities_readiness_fails_closed_at_age_equals_max_age_ms_boundary() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["legs"]["ibkr:AAPL.NASDAQ"]["age_ms"] = 10_000

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=signals_payload,
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is False
    assert result.checks["signals"].details["over_age_signal_legs"] == ["ibkr:AAPL.NASDAQ"]
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "aapl_tradexyz_makerv3",
    ]


def test_evaluate_equities_readiness_accepts_live_shaped_maker_leg_key() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    maker_leg = first_strategy["legs"].pop("hyperliquid:xyz:AAPL-USD-PERP.HYPERLIQUID")
    first_strategy["legs"]["hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID"] = maker_leg

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=signals_payload,
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is True
    assert result.checks["signals"].details["over_age_signal_legs"] == []


def test_evaluate_equities_readiness_ignores_unrelated_stale_legs_in_live_rows() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["debug"]["md_health"]["stale_legs"] = [
        "hyperliquid:XYZ:MSFT-USD-PERP.HYPERLIQUID",
        "ibkr:MSFT.NASDAQ",
    ]

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=signals_payload,
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is True
    assert result.checks["signals"].details["stale_signal_legs"] == []
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == []


def test_evaluate_equities_readiness_prefers_live_role_map_ref_leg_key() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["maker_role_map"]["ref_leg"] = "ibkr:AAPL.NASDAQ"
    first_strategy["state"]["maker_role_map"]["ref_leg"] = "nasdaq:AAPL.NASDAQ"

    mismatched_contracts = (
        StrategyContractEntry(
            strategy_id="aapl_tradexyz_makerv3",
            portfolio_asset_id="AAPL",
            maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            reference_instrument_id="AAPL.SMART",
            execution_account_scope_id="hyperliquid.xyz.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
        _strategy_contracts()[1],
    )

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=mismatched_contracts,
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=signals_payload,
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is True
    assert result.checks["signals"].details["over_age_signal_legs"] == []
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == []


def test_equities_readiness_wrapper_and_runbook_document_the_host_local_gate() -> None:
    repo_root = _repo_root()
    script = _read(repo_root / "ops/scripts/deploy/check_equities_live_readiness.sh")
    readme = _read(repo_root / "deploy/equities/README.md")

    assert "nautilus_trader.flux.runners.equities.readiness" in script
    assert 'EQUITIES_READINESS_CONFIG_PATH="${EQUITIES_READINESS_CONFIG_PATH:-' in script
    assert 'EQUITIES_READINESS_COMMON_ENV_PATH="${EQUITIES_READINESS_COMMON_ENV_PATH:-/etc/flux/common.env}"' in script
    assert "sudo -n" in script
    assert "EQUITIES_REDIS_HOST" in script
    assert "EQUITIES_READINESS_API_BASE_URL" in script
    assert "EQUITIES_READY_MAX_STALE_SIGNAL_LEGS" in script
    assert "EQUITIES_READY_MAX_UNHEALTHY_STRATEGIES" in script
    assert "EQUITIES_READY_IGNORE_REFERENCE_FRESHNESS_OUTSIDE_REGULAR_SESSION" in script
    assert "--ignore-reference-freshness-outside-regular-session" in script
    assert "profile_account_projection" in readme
    assert "check_equities_live_readiness.sh" in readme
    assert "/api/v1/signals?profile=equities" in readme
    assert "/api/v1/balances?profile=equities" in readme
    assert "regular US session" in readme
