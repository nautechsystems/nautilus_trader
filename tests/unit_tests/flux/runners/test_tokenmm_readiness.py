from __future__ import annotations


def _signal_payload(
    *,
    strategy_id: str,
    state: str = "running",
    state_stale: bool = False,
    signal_state_age_ms: int = 1_000,
    mode: str = "ON",
    running: bool = True,
    blocked: bool = False,
    tradeable: bool = True,
    balances_ok: bool = True,
    local_qty_base: float | None = 1.0,
    global_qty_base: float | None = 2.0,
    global_qty_base_complete: bool = True,
    maker_quote_status: dict[str, int] | None = None,
) -> dict[str, object]:
    quote_status = maker_quote_status
    if quote_status is None:
        quote_status = {
            "bid_open": 1,
            "ask_open": 1,
            "bid_depth": 1,
            "ask_depth": 1,
        }
    return {
        "id": strategy_id,
        "mode": mode,
        "running": running,
        "blocked": blocked,
        "tradeable": tradeable,
        "balances_ok": balances_ok,
        "local_qty_base": local_qty_base,
        "local_qty": local_qty_base,
        "global_qty_base": global_qty_base,
        "global_qty": global_qty_base,
        "global_qty_base_complete": global_qty_base_complete,
        "global_qty_complete": global_qty_base_complete,
        "maker_quote_status": quote_status,
        "state": {"state": state},
        "debug": {
            "md_health": {
                "state_stale": state_stale,
                "signal_state_age_ms": signal_state_age_ms,
                "stale_legs": [],
            },
        },
    }


def test_evaluate_tokenmm_readiness_marks_stale_state_stream_not_ready() -> None:
    from flux.runners.tokenmm.readiness import evaluate_tokenmm_readiness

    result = evaluate_tokenmm_readiness(
        required_strategy_ids=("plumeusdt_bybit_perp_makerv3",),
        signals_payload={
            "server_ts_ms": 1_700_000_031_000,
            "strategies": [_signal_payload(strategy_id="plumeusdt_bybit_perp_makerv3")],
        },
        state_streams_by_strategy_id={
            "plumeusdt_bybit_perp_makerv3": {
                "key": "flux:v1:in:stream:live:plumeusdt_bybit_perp_makerv3:flux.makerv3.state",
                "entry_id": "1700000000000-0",
                "ts_ms": 1_700_000_000_000,
                "age_ms": 31_000,
                "present": True,
            },
        },
        now_ms_value=1_700_000_031_000,
    )

    assert result.ok is False
    assert result.checks["state_stream_freshness"].ok is False
    assert result.summary["stale_state_stream_strategy_ids"] == ["plumeusdt_bybit_perp_makerv3"]
    assert result.summary["failed_checks"] == ["state_stream_freshness"]


def test_evaluate_tokenmm_readiness_marks_stale_signal_state_not_ready() -> None:
    from flux.runners.tokenmm.readiness import evaluate_tokenmm_readiness

    result = evaluate_tokenmm_readiness(
        required_strategy_ids=("plumeusdt_bybit_perp_makerv3",),
        signals_payload={
            "server_ts_ms": 1_700_000_005_000,
            "strategies": [
                _signal_payload(
                    strategy_id="plumeusdt_bybit_perp_makerv3",
                    state_stale=True,
                    signal_state_age_ms=45_000,
                ),
            ],
        },
        state_streams_by_strategy_id={
            "plumeusdt_bybit_perp_makerv3": {
                "key": "flux:v1:in:stream:live:plumeusdt_bybit_perp_makerv3:flux.makerv3.state",
                "entry_id": "1700000004000-0",
                "ts_ms": 1_700_000_004_000,
                "age_ms": 1_000,
                "present": True,
            },
        },
        now_ms_value=1_700_000_005_000,
    )

    assert result.ok is False
    assert result.checks["signals"].ok is False
    assert result.summary["stale_signal_strategy_ids"] == ["plumeusdt_bybit_perp_makerv3"]
    assert result.summary["failed_checks"] == ["signals"]


def test_evaluate_tokenmm_readiness_marks_blocked_reconciliation_not_ready() -> None:
    from flux.runners.tokenmm.readiness import evaluate_tokenmm_readiness

    result = evaluate_tokenmm_readiness(
        required_strategy_ids=("plumeusdt_bybit_perp_makerv3",),
        signals_payload={
            "server_ts_ms": 1_700_000_005_000,
            "strategies": [
                _signal_payload(
                    strategy_id="plumeusdt_bybit_perp_makerv3",
                    state="blocked_reconciliation",
                ),
            ],
        },
        state_streams_by_strategy_id={
            "plumeusdt_bybit_perp_makerv3": {
                "key": "flux:v1:in:stream:live:plumeusdt_bybit_perp_makerv3:flux.makerv3.state",
                "entry_id": "1700000004000-0",
                "ts_ms": 1_700_000_004_000,
                "age_ms": 1_000,
                "present": True,
            },
        },
        now_ms_value=1_700_000_005_000,
    )

    assert result.ok is False
    assert result.checks["signals"].ok is False
    assert result.checks["signals"].details["blocked_reconciliation_strategy_ids"] == [
        "plumeusdt_bybit_perp_makerv3",
    ]
    assert result.summary["blocked_reconciliation_strategy_ids"] == [
        "plumeusdt_bybit_perp_makerv3",
    ]
    assert result.summary["failed_checks"] == ["signals"]


def test_evaluate_tokenmm_readiness_marks_startup_bot_off_signal_not_ready() -> None:
    from flux.runners.tokenmm.readiness import OPERATOR_SURFACE
    from flux.runners.tokenmm.readiness import evaluate_tokenmm_readiness

    result = evaluate_tokenmm_readiness(
        required_strategy_ids=("plumeusdt_binance_perp_makerv3",),
        signals_payload={
            "server_ts_ms": 1_700_000_005_000,
            "strategies": [
                _signal_payload(
                    strategy_id="plumeusdt_binance_perp_makerv3",
                    state="startup_bot_off",
                    mode="OFF",
                    blocked=True,
                    tradeable=False,
                ),
            ],
        },
        state_streams_by_strategy_id={
            "plumeusdt_binance_perp_makerv3": {
                "key": "flux:v1:in:stream:live:plumeusdt_binance_perp_makerv3:flux.makerv3.state",
                "entry_id": "1700000004000-0",
                "ts_ms": 1_700_000_004_000,
                "age_ms": 1_000,
                "present": True,
            },
        },
        now_ms_value=1_700_000_005_000,
    )

    assert result.ok is False
    assert result.checks[OPERATOR_SURFACE].ok is False
    assert result.checks[OPERATOR_SURFACE].details["non_on_mode_strategy_ids"] == [
        "plumeusdt_binance_perp_makerv3",
    ]
    assert result.checks[OPERATOR_SURFACE].details["blocked_signal_strategy_ids"] == [
        "plumeusdt_binance_perp_makerv3",
    ]
    assert result.checks[OPERATOR_SURFACE].details["non_tradeable_strategy_ids"] == [
        "plumeusdt_binance_perp_makerv3",
    ]
    assert result.summary["failed_checks"] == [OPERATOR_SURFACE]


def test_evaluate_tokenmm_readiness_marks_missing_signal_operator_fields_not_ready() -> None:
    from flux.runners.tokenmm.readiness import OPERATOR_SURFACE
    from flux.runners.tokenmm.readiness import evaluate_tokenmm_readiness

    result = evaluate_tokenmm_readiness(
        required_strategy_ids=("plumeusdt_bitget_spot_makerv3",),
        signals_payload={
            "server_ts_ms": 1_700_000_005_000,
            "strategies": [
                _signal_payload(
                    strategy_id="plumeusdt_bitget_spot_makerv3",
                    balances_ok=False,
                    local_qty_base=None,
                    global_qty_base=None,
                    global_qty_base_complete=False,
                    maker_quote_status={},
                ),
            ],
        },
        state_streams_by_strategy_id={
            "plumeusdt_bitget_spot_makerv3": {
                "key": "flux:v1:in:stream:live:plumeusdt_bitget_spot_makerv3:flux.makerv3.state",
                "entry_id": "1700000004000-0",
                "ts_ms": 1_700_000_004_000,
                "age_ms": 1_000,
                "present": True,
            },
        },
        now_ms_value=1_700_000_005_000,
    )

    assert result.ok is False
    assert result.checks[OPERATOR_SURFACE].ok is False
    assert result.checks[OPERATOR_SURFACE].details["balance_not_ready_strategy_ids"] == [
        "plumeusdt_bitget_spot_makerv3",
    ]
    assert result.checks[OPERATOR_SURFACE].details["missing_local_qty_strategy_ids"] == [
        "plumeusdt_bitget_spot_makerv3",
    ]
    assert result.checks[OPERATOR_SURFACE].details["missing_global_qty_strategy_ids"] == [
        "plumeusdt_bitget_spot_makerv3",
    ]
    assert result.checks[OPERATOR_SURFACE].details["incomplete_global_qty_strategy_ids"] == [
        "plumeusdt_bitget_spot_makerv3",
    ]
    assert result.checks[OPERATOR_SURFACE].details["missing_quote_status_strategy_ids"] == [
        "plumeusdt_bitget_spot_makerv3",
    ]
    assert result.summary["failed_checks"] == [OPERATOR_SURFACE]
