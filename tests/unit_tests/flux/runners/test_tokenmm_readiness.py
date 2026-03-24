from __future__ import annotations


def _signal_payload(
    *,
    strategy_id: str,
    state_stale: bool = False,
    signal_state_age_ms: int = 1_000,
) -> dict[str, object]:
    return {
        "id": strategy_id,
        "state": {"state": "bot_off"},
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
