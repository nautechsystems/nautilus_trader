from __future__ import annotations

from datetime import datetime
from datetime import timezone
from pathlib import Path

from flux.api.payloads import ContractCatalogEntry
from flux.api.payloads import StrategyMetadata
from flux.api.payloads import build_legs_payload
from flux.api.payloads import build_signals_payload
from flux.common.account_scopes import AccountScopeConfig
from flux.common.controller_scopes import ControllerScopeConfig
from flux.common.strategy_contracts import StrategyContractEntry


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _strategy_contracts() -> tuple[StrategyContractEntry, ...]:
    return (
        StrategyContractEntry(
            strategy_id="aapl_tradexyz_makerv4",
            portfolio_asset_id="AAPL",
            maker_venue="HYPERLIQUID",
            maker_symbol="AAPL",
            market_type="perp",
            maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            reference_instrument_id="AAPL.NASDAQ",
            execution_account_scope_id="hyperliquid.xyz.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
        StrategyContractEntry(
            strategy_id="msft_tradexyz_makerv4",
            portfolio_asset_id="MSFT",
            maker_venue="HYPERLIQUID",
            maker_symbol="MSFT",
            market_type="perp",
            maker_instrument_id="xyz:MSFT-USD-PERP.HYPERLIQUID",
            reference_instrument_id="MSFT.NASDAQ",
            execution_account_scope_id="hyperliquid.xyz.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
    )


def _split_strategy_contracts() -> tuple[StrategyContractEntry, ...]:
    return (
        StrategyContractEntry(
            strategy_id="aapl_tradexyz_maker",
            portfolio_asset_id="AAPL",
            maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            reference_instrument_id="AAPL.NASDAQ",
            execution_account_scope_id="hyperliquid.xyz.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
        StrategyContractEntry(
            strategy_id="aapl_tradexyz_taker",
            portfolio_asset_id="AAPL",
            maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            reference_instrument_id="AAPL.NASDAQ",
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


def _controller_scopes() -> tuple[ControllerScopeConfig, ...]:
    return (
        ControllerScopeConfig(
            controller_scope_id="equities.ibkr.hedge.main",
            profile_id="equities",
            writer_account_scope_id="ibkr.hedge.main",
            account_scope_ids=("ibkr.hedge.main",),
            canary=True,
        ),
    )


def _healthy_signal_payload() -> dict[str, object]:
    return {
        "server_ts_ms": 1_700_000_000_500,
        "strategies": [
            {
                "id": "aapl_tradexyz_makerv4",
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
                "id": "msft_tradexyz_makerv4",
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
        "aapl_tradexyz_makerv4": {
            "strategy_id": "aapl_tradexyz_makerv4",
            "portfolio_id": "equities",
            "base_currency": "AAPL",
            "local_qty_base": "10",
            "ts_ms": 1_700_000_000_000,
        },
        "msft_tradexyz_makerv4": {
            "strategy_id": "msft_tradexyz_makerv4",
            "portfolio_id": "equities",
            "base_currency": "MSFT",
            "local_qty_base": "5",
            "ts_ms": 1_700_000_000_000,
        },
    }


def _healthy_ibkr_reference_publisher_status_payload() -> dict[str, object]:
    return {
        "profile_id": "equities",
        "account_scope_id": "ibkr.reference.main",
        "service_id": "ibkr_reference_publisher",
        "state": "publishing",
        "connected": True,
        "instrument_count": 2,
        "instrument_status": {
            "AAPL.NASDAQ": {
                "state": "healthy",
                "route": "SMART",
                "age_ms": 50,
                "ts_event_ms": 1_700_000_000_450,
            },
            "MSFT.NASDAQ": {
                "state": "healthy",
                "route": "SMART",
                "age_ms": 60,
                "ts_event_ms": 1_700_000_000_440,
            },
        },
        "last_success_ts_ms": 1_700_000_000_450,
        "last_error_type": None,
        "last_error_message": None,
        "stale_after_ms": 1_500,
        "ts_ms": 1_700_000_000_500,
    }


def _split_healthy_signal_payload() -> dict[str, object]:
    return {
        "server_ts_ms": 1_700_000_000_500,
        "strategies": [
            {
                "id": "aapl_tradexyz_maker",
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
                "id": "aapl_tradexyz_taker",
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
                    "ibkr:AAPL.NASDAQ": {"age_ms": 60},
                    "hyperliquid:xyz:AAPL-USD-PERP.HYPERLIQUID": {"age_ms": 30},
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


def _split_healthy_component_payloads() -> dict[str, dict[str, object]]:
    return {
        "aapl_tradexyz_maker": {
            "strategy_id": "aapl_tradexyz_maker",
            "portfolio_id": "equities",
            "base_currency": "AAPL",
            "local_qty_base": "10",
            "ts_ms": 1_700_000_000_000,
        },
        "aapl_tradexyz_taker": {
            "strategy_id": "aapl_tradexyz_taker",
            "portfolio_id": "equities",
            "base_currency": "AAPL",
            "local_qty_base": "10",
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
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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


def test_evaluate_equities_readiness_ignores_controller_owned_writer_scopes_for_projections() -> (
    None
):
    from flux.runners.equities.readiness import evaluate_equities_readiness

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        controller_scopes=_controller_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=_healthy_signal_payload(),
        projection_payloads_by_scope_id={
            "ibkr.reference.main": _healthy_projection_payloads()["ibkr.reference.main"],
        },
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is True
    assert result.summary["expected_projection_scope_ids"] == [
        "ibkr.reference.main",
    ]
    assert result.checks["profile_account_projections"].ok is True
    assert result.checks["ibkr_auth"].ok is True


def test_evaluate_equities_readiness_allows_multiple_routes_for_same_portfolio_asset() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    strategy_contracts = (
        StrategyContractEntry(
            strategy_id="pltr_tradexyz_makerv4",
            portfolio_asset_id="PLTR",
            maker_venue="HYPERLIQUID",
            maker_symbol="PLTR",
            market_type="perp",
            maker_instrument_id="xyz:PLTR-USD-PERP.HYPERLIQUID",
            reference_instrument_id="PLTR.NASDAQ",
            execution_account_scope_id="hyperliquid.xyz.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
        StrategyContractEntry(
            strategy_id="pltr_binance_perp_makerv4",
            portfolio_asset_id="PLTR",
            maker_venue="BINANCE_PERP",
            maker_symbol="PLTRUSDT",
            market_type="perp",
            maker_instrument_id="PLTRUSDT-PERP.BINANCE_PERP",
            reference_instrument_id="PLTR.NASDAQ",
            execution_account_scope_id="binance.futures.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
    )
    account_scopes = (
        AccountScopeConfig(
            scope_id="hyperliquid.xyz.main",
            provider="hyperliquid",
            venue="HYPERLIQUID",
        ),
        AccountScopeConfig(
            scope_id="binance.futures.main",
            provider="binance",
            venue="BINANCE_PERP",
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
    signals_payload = {
        "server_ts_ms": 1_700_000_000_500,
        "strategies": [
            {
                "id": "pltr_tradexyz_makerv4",
                "params": {"max_age_ms": "10000"},
                "maker_role_map": {
                    "maker_leg": "hyperliquid:XYZ:PLTR-USD-PERP.HYPERLIQUID",
                    "ref_leg": "ibkr:PLTR.NASDAQ",
                },
                "state": {
                    "state": "bot_off",
                    "maker_role_map": {
                        "maker_leg": "hyperliquid:XYZ:PLTR-USD-PERP.HYPERLIQUID",
                        "ref_leg": "nasdaq:PLTR.NASDAQ",
                    },
                },
                "legs": {
                    "ibkr:PLTR.NASDAQ": {"age_ms": 50},
                    "hyperliquid:xyz:PLTR-USD-PERP.HYPERLIQUID": {"age_ms": 25},
                },
                "debug": {
                    "md_health": {
                        "stale_legs": [],
                        "state_stale": False,
                    },
                },
            },
            {
                "id": "pltr_binance_perp_makerv4",
                "params": {"max_age_ms": "10000"},
                "maker_role_map": {
                    "maker_leg": "binance_perp:PLTRUSDT-PERP.BINANCE_PERP",
                    "ref_leg": "ibkr:PLTR.NASDAQ",
                },
                "state": {
                    "state": "bot_off",
                    "maker_role_map": {
                        "maker_leg": "binance_perp:PLTRUSDT-PERP.BINANCE_PERP",
                        "ref_leg": "nasdaq:PLTR.NASDAQ",
                    },
                },
                "legs": {
                    "ibkr:PLTR.NASDAQ": {"age_ms": 55},
                    "binance_perp:PLTRUSDT-PERP.BINANCE_PERP": {"age_ms": 20},
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
    component_payloads = {
        "pltr_tradexyz_makerv4": {
            "strategy_id": "pltr_tradexyz_makerv4",
            "portfolio_id": "equities",
            "base_currency": "PLTR",
            "local_qty_base": "10",
            "ts_ms": 1_700_000_000_000,
        },
        "pltr_binance_perp_makerv4": {
            "strategy_id": "pltr_binance_perp_makerv4",
            "portfolio_id": "equities",
            "base_currency": "PLTR",
            "local_qty_base": "12.5",
            "ts_ms": 1_700_000_000_000,
        },
    }
    projection_payloads = _healthy_projection_payloads() | {
        "binance.futures.main": {
            "account_scope_ids": ["binance.futures.main"],
            "rows": [{"exchange": "binance_perp", "asset": "USDT", "total": "750"}],
            "server_ts_ms": 1_700_000_000_050,
        },
    }

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=strategy_contracts,
        account_scopes=account_scopes,
        required_strategy_ids=("pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=signals_payload,
        projection_payloads_by_scope_id=projection_payloads,
        component_payloads_by_strategy_id=component_payloads,
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is True
    assert result.summary["expected_projection_scope_ids"] == [
        "binance.futures.main",
        "ibkr.hedge.main",
        "ibkr.reference.main",
    ]
    assert result.summary["healthy_strategy_count"] == 2
    assert result.checks["component_keys"].details["missing_strategy_ids"] == []
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == []


def test_evaluate_equities_readiness_requires_binance_projection_for_binance_routes() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    strategy_contracts = (
        StrategyContractEntry(
            strategy_id="pltr_binance_perp_makerv4",
            portfolio_asset_id="PLTR",
            maker_venue="BINANCE_PERP",
            maker_symbol="PLTRUSDT",
            market_type="perp",
            maker_instrument_id="PLTRUSDT-PERP.BINANCE_PERP",
            reference_instrument_id="PLTR.NASDAQ",
            execution_account_scope_id="binance.futures.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
    )
    account_scopes = (
        AccountScopeConfig(
            scope_id="binance.futures.main",
            provider="binance",
            venue="BINANCE_PERP",
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
    signals_payload = {
        "server_ts_ms": 1_700_000_000_500,
        "strategies": [
            {
                "id": "pltr_binance_perp_makerv4",
                "params": {"max_age_ms": "10000"},
                "maker_role_map": {
                    "maker_leg": "binance_perp:PLTRUSDT-PERP.BINANCE_PERP",
                    "ref_leg": "ibkr:PLTR.NASDAQ",
                },
                "state": {
                    "state": "bot_off",
                    "maker_role_map": {
                        "maker_leg": "binance_perp:PLTRUSDT-PERP.BINANCE_PERP",
                        "ref_leg": "nasdaq:PLTR.NASDAQ",
                    },
                },
                "legs": {
                    "ibkr:PLTR.NASDAQ": {"age_ms": 55},
                    "binance_perp:PLTRUSDT-PERP.BINANCE_PERP": {"age_ms": 20},
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
    component_payloads = {
        "pltr_binance_perp_makerv4": {
            "strategy_id": "pltr_binance_perp_makerv4",
            "portfolio_id": "equities",
            "base_currency": "PLTR",
            "local_qty_base": "12.5",
            "ts_ms": 1_700_000_000_000,
        },
    }

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=strategy_contracts,
        account_scopes=account_scopes,
        required_strategy_ids=("pltr_binance_perp_makerv4",),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=signals_payload,
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=component_payloads,
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is False
    assert result.summary["expected_projection_scope_ids"] == [
        "binance.futures.main",
        "ibkr.hedge.main",
        "ibkr.reference.main",
    ]
    assert result.checks["profile_account_projections"].details["missing_scope_ids"] == [
        "binance.futures.main",
    ]


def test_evaluate_equities_readiness_tracks_signal_health_per_route_inside_stock_netted_portfolio() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    strategy_contracts = (
        StrategyContractEntry(
            strategy_id="pltr_tradexyz_makerv4",
            portfolio_asset_id="PLTR",
            maker_venue="HYPERLIQUID",
            maker_symbol="PLTR",
            market_type="perp",
            maker_instrument_id="xyz:PLTR-USD-PERP.HYPERLIQUID",
            reference_instrument_id="PLTR.NASDAQ",
            execution_account_scope_id="hyperliquid.xyz.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
        StrategyContractEntry(
            strategy_id="pltr_binance_perp_makerv4",
            portfolio_asset_id="PLTR",
            maker_venue="BINANCE_PERP",
            maker_symbol="PLTRUSDT",
            market_type="perp",
            maker_instrument_id="PLTRUSDT-PERP.BINANCE_PERP",
            reference_instrument_id="PLTR.NASDAQ",
            execution_account_scope_id="binance.futures.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
    )
    account_scopes = (
        AccountScopeConfig(
            scope_id="hyperliquid.xyz.main",
            provider="hyperliquid",
            venue="HYPERLIQUID",
        ),
        AccountScopeConfig(
            scope_id="binance.futures.main",
            provider="binance",
            venue="BINANCE_PERP",
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
    signals_payload = {
        "server_ts_ms": 1_700_000_000_500,
        "strategies": [
            {
                "id": "pltr_tradexyz_makerv4",
                "params": {"max_age_ms": "10000"},
                "maker_role_map": {
                    "maker_leg": "hyperliquid:XYZ:PLTR-USD-PERP.HYPERLIQUID",
                    "ref_leg": "ibkr:PLTR.NASDAQ",
                },
                "state": {
                    "state": "bot_off",
                    "maker_role_map": {
                        "maker_leg": "hyperliquid:XYZ:PLTR-USD-PERP.HYPERLIQUID",
                        "ref_leg": "nasdaq:PLTR.NASDAQ",
                    },
                },
                "legs": {
                    "ibkr:PLTR.NASDAQ": {"age_ms": 50},
                    "hyperliquid:xyz:PLTR-USD-PERP.HYPERLIQUID": {"age_ms": 25},
                },
                "debug": {
                    "md_health": {
                        "stale_legs": [],
                        "state_stale": False,
                    },
                },
            },
            {
                "id": "pltr_binance_perp_makerv4",
                "params": {"max_age_ms": "10000"},
                "maker_role_map": {
                    "maker_leg": "binance_perp:PLTRUSDT-PERP.BINANCE_PERP",
                    "ref_leg": "ibkr:PLTR.NASDAQ",
                },
                "state": {
                    "state": "bot_off",
                    "maker_role_map": {
                        "maker_leg": "binance_perp:PLTRUSDT-PERP.BINANCE_PERP",
                        "ref_leg": "nasdaq:PLTR.NASDAQ",
                    },
                },
                "legs": {
                    "ibkr:PLTR.NASDAQ": {"age_ms": 55},
                    "binance_perp:PLTRUSDT-PERP.BINANCE_PERP": {"age_ms": 20_001},
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
    component_payloads = {
        "pltr_tradexyz_makerv4": {
            "strategy_id": "pltr_tradexyz_makerv4",
            "portfolio_id": "equities",
            "base_currency": "PLTR",
            "local_qty_base": "10",
            "ts_ms": 1_700_000_000_000,
        },
        "pltr_binance_perp_makerv4": {
            "strategy_id": "pltr_binance_perp_makerv4",
            "portfolio_id": "equities",
            "base_currency": "PLTR",
            "local_qty_base": "5",
            "ts_ms": 1_700_000_000_000,
        },
    }

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=strategy_contracts,
        account_scopes=account_scopes,
        required_strategy_ids=("pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=signals_payload,
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=component_payloads,
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is False
    assert result.checks["balances"].ok is True
    assert result.checks["component_keys"].ok is True
    assert result.checks["signals"].ok is False
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == ["pltr_binance_perp_makerv4"]
    assert result.checks["ibkr_auth"].ok is True
    assert result.summary["healthy_strategy_count"] == 1


def test_evaluate_equities_readiness_reports_same_stock_routes_per_strategy() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    strategy_contracts = (
        StrategyContractEntry(
            strategy_id="pltr_tradexyz_makerv4",
            portfolio_asset_id="PLTR",
            maker_venue="HYPERLIQUID",
            maker_symbol="PLTR",
            market_type="perp",
            maker_instrument_id="xyz:PLTR-USD-PERP.HYPERLIQUID",
            reference_instrument_id="PLTR.NASDAQ",
            execution_account_scope_id="hyperliquid.xyz.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
        StrategyContractEntry(
            strategy_id="pltr_binance_perp_makerv4",
            portfolio_asset_id="PLTR",
            maker_venue="BINANCE_PERP",
            maker_symbol="PLTRUSDT",
            market_type="perp",
            maker_instrument_id="PLTRUSDT-PERP.BINANCE_PERP",
            reference_instrument_id="PLTR.NASDAQ",
            execution_account_scope_id="binance.futures.main",
            reference_account_scope_id="ibkr.reference.main",
            hedge_account_scope_id="ibkr.hedge.main",
        ),
    )
    account_scopes = (
        AccountScopeConfig(
            scope_id="hyperliquid.xyz.main",
            provider="hyperliquid",
            venue="HYPERLIQUID",
        ),
        AccountScopeConfig(
            scope_id="binance.futures.main",
            provider="binance",
            venue="BINANCE_PERP",
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
    signals_payload = {
        "server_ts_ms": 1_700_000_000_500,
        "strategies": [
            {
                "id": "pltr_tradexyz_makerv4",
                "params": {"max_age_ms": "10000"},
                "maker_role_map": {
                    "maker_leg": "hyperliquid:XYZ:PLTR-USD-PERP.HYPERLIQUID",
                    "ref_leg": "ibkr:PLTR.NASDAQ",
                },
                "state": {"state": "bot_off"},
                "legs": {
                    "ibkr:PLTR.NASDAQ": {"age_ms": 50},
                    "hyperliquid:xyz:PLTR-USD-PERP.HYPERLIQUID": {"age_ms": 25},
                },
                "debug": {"md_health": {"stale_legs": [], "state_stale": False}},
            },
            {
                "id": "pltr_binance_perp_makerv4",
                "params": {"max_age_ms": "10000"},
                "maker_role_map": {
                    "maker_leg": "binance_perp:PLTRUSDT-PERP.BINANCE_PERP",
                    "ref_leg": "ibkr:PLTR.NASDAQ",
                },
                "state": {"state": "bot_off"},
                "legs": {
                    "ibkr:PLTR.NASDAQ": {"age_ms": 55},
                    "binance_perp:PLTRUSDT-PERP.BINANCE_PERP": {"age_ms": 20},
                },
                "debug": {
                    "md_health": {
                        "stale_legs": ["binance_perp:PLTRUSDT-PERP.BINANCE_PERP"],
                        "state_stale": False,
                    },
                },
            },
        ],
    }
    component_payloads = {
        "pltr_tradexyz_makerv4": {
            "strategy_id": "pltr_tradexyz_makerv4",
            "portfolio_id": "equities",
            "base_currency": "PLTR",
            "local_qty_base": "10",
            "ts_ms": 1_700_000_000_000,
        },
        "pltr_binance_perp_makerv4": {
            "strategy_id": "pltr_binance_perp_makerv4",
            "portfolio_id": "equities",
            "base_currency": "PLTR",
            "local_qty_base": "-4",
            "ts_ms": 1_700_000_000_000,
        },
    }

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=strategy_contracts,
        account_scopes=account_scopes,
        required_strategy_ids=("pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=signals_payload,
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=component_payloads,
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is False
    assert result.summary["healthy_strategy_count"] == 1
    assert result.checks["balances"].ok is True
    assert result.checks["component_keys"].ok is True
    assert result.checks["signals"].details["required_strategy_ids"] == [
        "pltr_tradexyz_makerv4",
        "pltr_binance_perp_makerv4",
    ]
    assert result.checks["signals"].details["stale_signal_legs"] == [
        "binance_perp:PLTRUSDT-PERP.BINANCE_PERP",
    ]
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "pltr_binance_perp_makerv4",
    ]
    assert result.checks["ibkr_auth"].ok is True
    assert result.checks["ibkr_auth"].details["unhealthy_strategy_ids"] == []


def test_evaluate_equities_readiness_fails_closed_for_live_blockers() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": True,
            "missing_required": ["msft_tradexyz_makerv4"],
        },
        signals_payload={
            "server_ts_ms": 1_700_000_000_500,
            "strategies": [
                {
                    "id": "aapl_tradexyz_makerv4",
                    "params": {"max_age_ms": "10000"},
                    "state": {"state": "bot_off"},
                    "legs": {
                        "ibkr:AAPL.NASDAQ": {"age_ms": 50},
                        "hyperliquid:xyz:AAPL-USD-PERP.HYPERLIQUID": {"age_ms": 25},
                    },
                    "debug": {"md_health": {"stale_legs": [], "state_stale": False}},
                },
                {
                    "id": "msft_tradexyz_makerv4",
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
            "aapl_tradexyz_makerv4": _healthy_component_payloads()["aapl_tradexyz_makerv4"],
            "msft_tradexyz_makerv4": None,
        },
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is False
    assert result.checks["balances"].details["missing_required"] == ["msft_tradexyz_makerv4"]
    assert result.checks["profile_account_projections"].details["missing_scope_ids"] == [
        "ibkr.hedge.main",
    ]
    assert result.checks["profile_account_projections"].details["stale_scope_ids"] == [
        "ibkr.reference.main",
    ]
    assert result.checks["component_keys"].details["missing_strategy_ids"] == [
        "msft_tradexyz_makerv4",
    ]
    assert result.checks["signals"].details["stale_signal_legs"] == ["ibkr:MSFT.NASDAQ"]
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "msft_tradexyz_makerv4",
    ]
    assert result.checks["ibkr_auth"].details["unhealthy_strategy_ids"] == [
        "msft_tradexyz_makerv4",
    ]


def test_evaluate_equities_readiness_thresholds_are_overridable() -> None:
    from flux.runners.equities.readiness import EquitiesReadinessThresholds
    from flux.runners.equities.readiness import evaluate_equities_readiness

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload={
            "server_ts_ms": 1_700_000_000_500,
            "strategies": [
                {
                    "id": "aapl_tradexyz_makerv4",
                    "params": {"max_age_ms": "10000"},
                    "state": {"state": "bot_off"},
                    "legs": {
                        "ibkr:AAPL.NASDAQ": {"age_ms": 50},
                        "hyperliquid:xyz:AAPL-USD-PERP.HYPERLIQUID": {"age_ms": 25},
                    },
                    "debug": {"md_health": {"stale_legs": [], "state_stale": False}},
                },
                {
                    "id": "msft_tradexyz_makerv4",
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
        "msft_tradexyz_makerv4",
    ]


def test_evaluate_equities_readiness_requires_both_same_asset_strategy_ids() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_split_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_maker", "aapl_tradexyz_taker"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=_split_healthy_signal_payload(),
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_split_healthy_component_payloads(),
        now_ms_value=1_700_000_000_500,
    )

    assert result.ok is True
    assert result.summary["required_strategy_ids"] == [
        "aapl_tradexyz_maker",
        "aapl_tradexyz_taker",
    ]
    assert result.summary["healthy_strategy_count"] == 2
    assert result.checks["component_keys"].details["expected_strategy_ids"] == [
        "aapl_tradexyz_maker",
        "aapl_tradexyz_taker",
    ]
    assert result.checks["component_keys"].details["missing_strategy_ids"] == []
    assert {
        payload["base_currency"]
        for payload in _split_healthy_component_payloads().values()
    } == {"AAPL"}


def test_evaluate_equities_readiness_uses_explicit_old_quote_state_from_signal_snapshot() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["maker_v4"] = {
        "quote_snapshot": {
            "maker_leg": {
                "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
            },
            "ref_leg": {
                "instrument_id": "AAPL.NASDAQ",
                "feed_state": "ok",
                "quote_state": "old",
                "pricing_usable": False,
                "hedge_usable": False,
            },
        },
    }

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
    assert result.checks["signals"].details["old_signal_legs"] == ["ibkr:AAPL.NASDAQ"]
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "aapl_tradexyz_makerv4",
    ]
    assert result.checks["ibkr_auth"].ok is False


def test_evaluate_equities_readiness_uses_explicit_missing_quote_state_from_signal_snapshot() -> (
    None
):
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["maker_v4"] = {
        "quote_snapshot": {
            "maker_leg": {
                "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
            },
            "ref_leg": {
                "instrument_id": "AAPL.NASDAQ",
                "feed_state": "ok",
                "quote_state": "missing",
                "pricing_usable": False,
                "hedge_usable": False,
            },
        },
    }

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
    assert result.checks["signals"].details["missing_signal_legs"] == ["ibkr:AAPL.NASDAQ"]
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "aapl_tradexyz_makerv4",
    ]
    assert result.checks["ibkr_auth"].ok is False


def test_evaluate_equities_readiness_uses_explicit_feed_down_state_from_signal_snapshot() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["maker_v4"] = {
        "quote_snapshot": {
            "maker_leg": {
                "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
            },
            "ref_leg": {
                "instrument_id": "AAPL.NASDAQ",
                "feed_state": "down",
                "quote_state": "fresh",
                "pricing_usable": False,
                "hedge_usable": False,
            },
        },
    }

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
    assert result.checks["signals"].details["feed_down_signal_legs"] == ["ibkr:AAPL.NASDAQ"]
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "aapl_tradexyz_makerv4",
    ]
    assert result.checks["ibkr_auth"].ok is False


def test_evaluate_equities_readiness_counts_recovering_quote_health_as_unhealthy() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    recovery_row = build_signals_payload(
        strategy_id="aapl_tradexyz_makerv4",
        metadata=StrategyMetadata(
            strategy_class="equities_maker",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="equities_maker",
            strategy_family="equities_maker",
            strategy_version="v1",
        ),
        state={
            "bot_on": True,
            "managed_orders": 1,
            "state": "running",
            "ts_ms": 1_700_000_000_450,
            "maker_role_map": {
                "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
                "ref_leg": "ibkr:AAPL.NASDAQ",
                "hedge_leg": "ibkr:AAPL.NASDAQ",
            },
            "maker_v4": {
                "quote_snapshot": {
                    "ts_ms": 1_700_000_000_450,
                    "maker_leg": {
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                        "bid": 190.10,
                        "ask": 190.20,
                        "ts_ms": 1_700_000_000_440,
                        "age_ms": 10,
                        "feed_state": "ok",
                        "quote_state": "fresh",
                        "pricing_usable": True,
                        "hedge_usable": True,
                    },
                    "ref_leg": {
                        "instrument_id": "AAPL.NASDAQ",
                        "bid": 189.90,
                        "ask": 190.00,
                        "ts_ms": 1_700_000_000_445,
                        "age_ms": 5,
                        "feed_state": "ok",
                        "quote_state": "fresh",
                        "pricing_usable": True,
                        "hedge_usable": True,
                        "recovery_state": "recovering",
                    },
                    "hedge_leg": {
                        "instrument_id": "AAPL.NASDAQ",
                        "route": "SMART",
                        "bid": 189.90,
                        "ask": 190.00,
                        "ts_ms": 1_700_000_000_445,
                        "age_ms": 5,
                        "feed_state": "ok",
                        "quote_state": "fresh",
                        "pricing_usable": True,
                        "hedge_usable": True,
                        "recovery_state": "recovering",
                    },
                },
            },
        },
        fv_row={"fv": 190.0},
        params={"qty": 1.0, "max_age_ms": 10_000, "max_ibkr_quote_age_ms": 1_000},
        balances=[],
        legs=build_legs_payload(
            contracts=(
                ContractCatalogEntry(
                    exchange="hyperliquid",
                    symbol="AAPL/USD",
                    instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
                ),
                ContractCatalogEntry(
                    exchange="ibkr",
                    symbol="AAPL/USD",
                    instrument_id="AAPL.NASDAQ",
                ),
            ),
            market_rows={
                "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID": {
                    "exchange": "hyperliquid",
                    "symbol": "AAPL/USD",
                    "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "bid": 190.10,
                    "ask": 190.20,
                    "ts_ms": 1_700_000_000_440,
                },
                "ibkr:AAPL.NASDAQ": {
                    "exchange": "ibkr",
                    "symbol": "AAPL/USD",
                    "instrument_id": "AAPL.NASDAQ",
                    "bid": 189.90,
                    "ask": 190.00,
                    "ts_ms": 1_700_000_000_445,
                },
            },
            now_ms_value=1_700_000_000_450,
        ),
    )

    signals_payload = _healthy_signal_payload()
    signals_payload["strategies"][0] = recovery_row

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
    assert result.checks["signals"].details["feed_down_signal_legs"] == ["ibkr:AAPL.NASDAQ"]
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "aapl_tradexyz_makerv4",
    ]
    assert result.summary["healthy_strategy_count"] == 1
    assert result.checks["ibkr_auth"].ok is False


def test_evaluate_equities_readiness_reads_shared_equities_arb_quote_snapshot_contract() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["equities_arb"] = {
        "quote_snapshot": {
            "maker_leg": {
                "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
            },
            "ref_leg": {
                "instrument_id": "AAPL.NASDAQ",
                "feed_state": "ok",
                "quote_state": "old",
                "pricing_usable": False,
                "hedge_usable": False,
            },
        },
    }

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
    assert result.checks["signals"].details["old_signal_legs"] == ["ibkr:AAPL.NASDAQ"]
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "aapl_tradexyz_makerv4",
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
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
        "aapl_tradexyz_makerv4",
    ]
    assert result.checks["ibkr_auth"].details["unhealthy_strategy_ids"] == [
        "aapl_tradexyz_makerv4",
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
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
        "aapl_tradexyz_makerv4",
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
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
        "aapl_tradexyz_makerv4",
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
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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


def test_evaluate_equities_readiness_ignores_stale_leg_markers_when_live_quote_snapshot_recovered() -> (
    None
):
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["debug"]["md_health"]["stale_legs"] = [
        "hyperliquid:xyz:AAPL-USD-PERP.HYPERLIQUID",
    ]
    first_strategy["equities_arb"] = {
        "quote_snapshot": {
            "maker_leg": {
                "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
                "age_ms": 0,
            },
            "ref_leg": {
                "instrument_id": "AAPL.NASDAQ",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
                "age_ms": 0,
            },
        },
    }

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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


def test_evaluate_equities_readiness_keeps_feed_healthy_when_strategy_blocked_only_by_wide_spread() -> (
    None
):
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["state"]["state"] = "blocked_spread_too_wide"
    first_strategy["equities_arb"] = {
        "quote_snapshot": {
            "maker_leg": {
                "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
                "age_ms": 0,
            },
            "ref_leg": {
                "instrument_id": "AAPL.NASDAQ",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
                "age_ms": 0,
            },
            "hedge_leg": {
                "instrument_id": "AAPL.NASDAQ",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
                "age_ms": 0,
            },
            "hedge_disabled_reason": "spread_too_wide",
            "hedge_ready": False,
        },
    }

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == []


def test_evaluate_equities_readiness_marks_locked_reference_quotes_unhealthy() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["state"]["state"] = "blocked_locked_or_crossed"
    first_strategy["equities_arb"] = {
        "quote_snapshot": {
            "maker_leg": {
                "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
                "age_ms": 0,
            },
            "ref_leg": {
                "instrument_id": "AAPL.NASDAQ",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
                "age_ms": 0,
            },
            "hedge_leg": {
                "instrument_id": "AAPL.NASDAQ",
                "feed_state": "ok",
                "quote_state": "fresh",
                "pricing_usable": True,
                "hedge_usable": True,
                "age_ms": 0,
            },
            "hedge_disabled_reason": "locked_or_crossed",
            "hedge_ready": False,
        },
    }

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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
    assert result.checks["signals"].details["unhealthy_strategy_ids"] == [
        "aapl_tradexyz_makerv4",
    ]


def test_evaluate_equities_readiness_prefers_live_role_map_ref_leg_key() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    signals_payload = _healthy_signal_payload()
    first_strategy = signals_payload["strategies"][0]
    first_strategy["maker_role_map"]["ref_leg"] = "ibkr:AAPL.NASDAQ"
    first_strategy["state"]["maker_role_map"]["ref_leg"] = "nasdaq:AAPL.NASDAQ"

    mismatched_contracts = (
        StrategyContractEntry(
            strategy_id="aapl_tradexyz_makerv4",
            portfolio_asset_id="AAPL",
            maker_venue="HYPERLIQUID",
            maker_symbol="AAPL",
            market_type="perp",
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
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
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


def test_evaluate_equities_readiness_requires_flux_owned_ibkr_reference_publisher_status() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=_healthy_signal_payload(),
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        publisher_status_payload=None,
        now_ms_value=1_700_000_000_500,
        require_ibkr_reference_publisher=True,
    )

    assert result.ok is False
    assert result.checks["ibkr_reference_publisher"].ok is False
    assert result.checks["ibkr_reference_publisher"].details["service_id"] == (
        "ibkr_reference_publisher"
    )
    assert result.checks["ibkr_reference_publisher"].details["missing"] is True


def test_evaluate_equities_readiness_accepts_healthy_flux_owned_ibkr_reference_publisher_status() -> (
    None
):
    from flux.runners.equities.readiness import evaluate_equities_readiness

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=_healthy_signal_payload(),
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        publisher_status_payload=_healthy_ibkr_reference_publisher_status_payload(),
        now_ms_value=1_700_000_000_500,
        require_ibkr_reference_publisher=True,
    )

    assert result.ok is True
    assert result.checks["ibkr_reference_publisher"].ok is True
    assert result.checks["ibkr_reference_publisher"].details["missing"] is False


def test_evaluate_equities_readiness_requires_publisher_to_be_actively_publishing() -> None:
    from flux.runners.equities.readiness import evaluate_equities_readiness

    publisher_status_payload = _healthy_ibkr_reference_publisher_status_payload()
    publisher_status_payload["state"] = "connected"
    publisher_status_payload["instrument_status"] = {}

    result = evaluate_equities_readiness(
        profile_id="equities",
        portfolio_id="equities",
        strategy_contracts=_strategy_contracts(),
        account_scopes=_account_scopes(),
        required_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
        balances_payload={
            "source": "portfolio_snapshot_v2",
            "degraded": False,
            "missing_required": [],
        },
        signals_payload=_healthy_signal_payload(),
        projection_payloads_by_scope_id=_healthy_projection_payloads(),
        component_payloads_by_strategy_id=_healthy_component_payloads(),
        publisher_status_payload=publisher_status_payload,
        now_ms_value=1_700_000_000_500,
        require_ibkr_reference_publisher=True,
    )

    assert result.ok is False
    assert result.checks["ibkr_reference_publisher"].ok is False
    assert result.checks["ibkr_reference_publisher"].details["state"] == "connected"


def test_equities_readiness_wrapper_and_runbook_document_the_host_local_gate() -> None:
    repo_root = _repo_root()
    script = _read(repo_root / "ops/scripts/deploy/check_equities_live_readiness.sh")
    readme = _read(repo_root / "deploy/equities/README.md")
    runbook = _read(repo_root / "docs/runbooks/equities-shared-node-cutover.md")

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
    assert "flux@equities-ibkr-reference-publisher.service" in readme
    assert "flux@equities-ibkr-reference-publisher.service" in runbook
    assert "chainsaw@md-ibkr-publisher.service" not in readme
    assert "chainsaw@md-ibkr-publisher.service" not in runbook


def test_equities_binance_perp_runbook_documents_multivenue_canary_sequence() -> None:
    runbook = _read(_repo_root() / "docs/runbooks/equities-binance-perp-market-making.md")

    assert "overlap-name canary" in runbook
    assert ("PLTR" in runbook) or ("TSLA" in runbook)
    assert "same-stock multi-venue netting" in runbook
    assert "inventory_by_asset" in runbook
    assert "source_strategy_ids" in runbook
    assert "Binance-only name canary" in runbook
    assert "MSTR" in runbook
    assert "newly discovered Binance-only routes" in runbook
    assert "/api/v1/balances?profile=equities" in runbook
