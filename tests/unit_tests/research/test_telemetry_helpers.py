from __future__ import annotations

import math

import pandas as pd
import pytest

from research.tokenmm.telemetry_helpers import compute_extended_markouts_from_fv_stream
from research.tokenmm.telemetry_helpers import compute_fill_time_edge_rows
from research.tokenmm.telemetry_helpers import lookup_benchmark_at_ts
from research.tokenmm.telemetry_helpers import parse_instrument_id
from research.tokenmm.telemetry_helpers import summarize_markouts
from research.tokenmm.telemetry_helpers import summarize_markouts_by_group
from research.tokenmm.telemetry_helpers import summarize_markouts_by_side
from research.tokenmm.telemetry_helpers import compute_fill_notional


def _sample_frames() -> tuple[pd.DataFrame, pd.DataFrame]:
    fills = pd.DataFrame(
        [
            {
                "trader_id": "t1",
                "event_id": "e1",
                "order_side": "BUY",
                "strategy_id": "s1",
                "instrument_id": "PLUMEUSDT-SPOT.BYBIT",
                "last_qty": 10.0,
                "last_px": 100.0,
            },
            {
                "trader_id": "t1",
                "event_id": "e2",
                "order_side": "SELL",
                "strategy_id": "s2",
                "instrument_id": "PLUMEUSDT-SPOT.BYBIT",
                "last_qty": 20.0,
                "last_px": 100.0,
            },
            {
                "trader_id": "t1",
                "event_id": "e3",
                "order_side": "BUY",
                "strategy_id": "s1",
                "instrument_id": "PLUME-USDT-SWAP.OKX",
                "last_qty": 5.0,
                "last_px": 100.0,
            },
        ],
    )
    markouts = pd.DataFrame(
        [
            {
                "trader_id": "t1",
                "event_id": "e1",
                "order_side": "BUY",
                "strategy_id": "s1",
                "horizon_s": 30,
                "fill_notional": 1_000.0,
                "markout_bps": 10.0,
                "resolution_status": "resolved",
            },
            {
                "trader_id": "t1",
                "event_id": "e1",
                "order_side": "BUY",
                "strategy_id": "s1",
                "horizon_s": 60,
                "fill_notional": 1_000.0,
                "markout_bps": 20.0,
                "resolution_status": "resolved",
            },
            {
                "trader_id": "t1",
                "event_id": "e2",
                "order_side": "SELL",
                "strategy_id": "s2",
                "horizon_s": 30,
                "fill_notional": 2_000.0,
                "markout_bps": -5.0,
                "resolution_status": "resolved",
            },
            {
                "trader_id": "t1",
                "event_id": "e2",
                "order_side": "SELL",
                "strategy_id": "s2",
                "horizon_s": 60,
                "fill_notional": 2_000.0,
                "markout_bps": 5.0,
                "resolution_status": "resolved",
            },
            {
                "trader_id": "t1",
                "event_id": "e3",
                "order_side": "BUY",
                "strategy_id": "s1",
                "horizon_s": 30,
                "fill_notional": 500.0,
                "markout_bps": math.nan,
                "resolution_status": "expired",
            },
        ],
    )
    return fills, markouts


def test_parse_instrument_id_returns_symbol_venue_and_product() -> None:
    spot = parse_instrument_id("PLUMEUSDT-SPOT.BYBIT")
    swap = parse_instrument_id("PLUME-USDT-SWAP.OKX")

    assert spot["symbol"] == "PLUMEUSDT"
    assert spot["venue"] == "BYBIT"
    assert spot["product"] == "SPOT"
    assert swap["symbol"] == "PLUME-USDT"
    assert swap["venue"] == "OKX"
    assert swap["product"] == "SWAP"


def test_compute_fill_notional_uses_absolute_price_times_quantity() -> None:
    frame = pd.DataFrame(
        [
            {"last_px": "0.01187", "last_qty": "1000"},
            {"last_px": 10.0, "last_qty": -3.0},
        ],
    )

    notionals = compute_fill_notional(frame)

    assert notionals.tolist() == pytest.approx([11.87, 30.0])


def test_summarize_markouts_returns_fill_count_gross_notional_and_horizon_columns() -> None:
    fills, markouts = _sample_frames()
    summary = summarize_markouts(fills=fills, markouts=markouts, horizons=(30, 60))

    assert summary.to_dict("records") == [
        {
            "fill_count": 3,
            "gross_notional": pytest.approx(3_500.0),
            "resolved_rows_30s": 2,
            "avg_markout_bps_30s": pytest.approx(2.5),
            "nw_markout_bps_30s": pytest.approx(0.0),
            "resolved_rows_60s": 2,
            "avg_markout_bps_60s": pytest.approx(12.5),
            "nw_markout_bps_60s": pytest.approx(10.0),
        },
    ]


def test_summarize_markouts_by_side_keeps_one_row_per_side() -> None:
    fills, markouts = _sample_frames()
    summary = summarize_markouts_by_side(fills=fills, markouts=markouts, horizons=(30, 60))
    records = summary.sort_values("order_side").to_dict("records")

    assert records == [
        {
            "order_side": "BUY",
            "fill_count": 2,
            "gross_notional": pytest.approx(1_500.0),
            "resolved_rows_30s": 1,
            "avg_markout_bps_30s": pytest.approx(10.0),
            "nw_markout_bps_30s": pytest.approx(10.0),
            "resolved_rows_60s": 1,
            "avg_markout_bps_60s": pytest.approx(20.0),
            "nw_markout_bps_60s": pytest.approx(20.0),
        },
        {
            "order_side": "SELL",
            "fill_count": 1,
            "gross_notional": pytest.approx(2_000.0),
            "resolved_rows_30s": 1,
            "avg_markout_bps_30s": pytest.approx(-5.0),
            "nw_markout_bps_30s": pytest.approx(-5.0),
            "resolved_rows_60s": 1,
            "avg_markout_bps_60s": pytest.approx(5.0),
            "nw_markout_bps_60s": pytest.approx(5.0),
        },
    ]


def test_summarize_markouts_by_group_keeps_requested_group_columns() -> None:
    fills, markouts = _sample_frames()
    summary = summarize_markouts_by_group(
        fills=fills,
        markouts=markouts,
        group_cols=("strategy_id", "venue"),
        horizons=(30,),
    )
    records = summary.sort_values(["strategy_id", "venue"]).to_dict("records")

    assert records[0] == {
        "strategy_id": "s1",
        "venue": "BYBIT",
        "fill_count": 1,
        "gross_notional": pytest.approx(1_000.0),
        "resolved_rows_30s": 1,
        "avg_markout_bps_30s": pytest.approx(10.0),
        "nw_markout_bps_30s": pytest.approx(10.0),
    }
    assert records[1]["strategy_id"] == "s1"
    assert records[1]["venue"] == "OKX"
    assert records[1]["fill_count"] == 1
    assert records[1]["gross_notional"] == pytest.approx(500.0)
    assert records[1]["resolved_rows_30s"] == 0
    assert math.isnan(records[1]["avg_markout_bps_30s"])
    assert math.isnan(records[1]["nw_markout_bps_30s"])
    assert records[2] == {
        "strategy_id": "s2",
        "venue": "BYBIT",
        "fill_count": 1,
        "gross_notional": pytest.approx(2_000.0),
        "resolved_rows_30s": 1,
        "avg_markout_bps_30s": pytest.approx(-5.0),
        "nw_markout_bps_30s": pytest.approx(-5.0),
    }


def test_lookup_benchmark_at_ts_supports_backward_and_forward_modes() -> None:
    fv_rows = pd.DataFrame(
        [
            {"strategy_id": "s1", "ts_ms": 1_000, "fv": 100.0},
            {"strategy_id": "s1", "ts_ms": 1_200, "fv": 101.0},
            {"strategy_id": "s1", "ts_ms": 1_800, "fv": 102.0},
        ],
    )
    rows = pd.DataFrame(
        [
            {"strategy_id": "s1", "fill_ts_ms": 1_150, "target_ts_ms": 1_150},
        ],
    )

    backward = lookup_benchmark_at_ts(
        rows=rows,
        fv_rows=fv_rows,
        benchmark_name="fv",
        timestamp_col="fill_ts_ms",
        direction="backward",
    )
    forward = lookup_benchmark_at_ts(
        rows=rows,
        fv_rows=fv_rows,
        benchmark_name="fv",
        timestamp_col="target_ts_ms",
        direction="forward",
    )

    assert backward.loc[0, "benchmark_px"] == pytest.approx(100.0)
    assert backward.loc[0, "benchmark_ts_ms"] == 1_000
    assert backward.loc[0, "lag_ms"] == 150
    assert backward.loc[0, "status"] == "ok"
    assert forward.loc[0, "benchmark_px"] == pytest.approx(101.0)
    assert forward.loc[0, "benchmark_ts_ms"] == 1_200
    assert forward.loc[0, "lag_ms"] == 50
    assert forward.loc[0, "status"] == "ok"


def test_compute_fill_time_edge_rows_uses_nearest_benchmark_with_lag_diagnostics() -> None:
    fills = pd.DataFrame(
        [
            {
                "trader_id": "t1",
                "event_id": "e1",
                "strategy_id": "s1",
                "order_side": "BUY",
                "fill_px_num": 100.0,
                "fill_qty_num": 2.0,
                "notional": 200.0,
                "ts_ms": 1_000,
            },
            {
                "trader_id": "t1",
                "event_id": "e2",
                "strategy_id": "s1",
                "order_side": "SELL",
                "fill_px_num": 100.0,
                "fill_qty_num": 1.0,
                "notional": 100.0,
                "ts_ms": 2_000,
            },
        ],
    )
    fv_rows = pd.DataFrame(
        [
            {"strategy_id": "s1", "ts_ms": 995, "fv": 101.0, "maker_mid": 100.5},
            {"strategy_id": "s1", "ts_ms": 1_995, "fv": 99.0, "maker_mid": 99.5},
        ],
    )

    rows = compute_fill_time_edge_rows(fills=fills, fv_rows=fv_rows)
    records = rows.sort_values(["event_id", "benchmark_name"]).to_dict("records")

    assert records == [
        {
            "trader_id": "t1",
            "event_id": "e1",
            "strategy_id": "s1",
            "fill_ts_ms": 1_000,
            "benchmark_ts_ms": 995,
            "lag_ms": 5,
            "benchmark_px": pytest.approx(101.0),
            "status": "ok",
            "benchmark_name": "fv",
            "edge_abs": pytest.approx(1.0),
            "edge_bps": pytest.approx(100.0),
        },
        {
            "trader_id": "t1",
            "event_id": "e1",
            "strategy_id": "s1",
            "fill_ts_ms": 1_000,
            "benchmark_ts_ms": 995,
            "lag_ms": 5,
            "benchmark_px": pytest.approx(100.5),
            "status": "ok",
            "benchmark_name": "maker_mid",
            "edge_abs": pytest.approx(0.5),
            "edge_bps": pytest.approx(50.0),
        },
        {
            "trader_id": "t1",
            "event_id": "e2",
            "strategy_id": "s1",
            "fill_ts_ms": 2_000,
            "benchmark_ts_ms": 1_995,
            "lag_ms": 5,
            "benchmark_px": pytest.approx(99.0),
            "status": "ok",
            "benchmark_name": "fv",
            "edge_abs": pytest.approx(1.0),
            "edge_bps": pytest.approx(100.0),
        },
        {
            "trader_id": "t1",
            "event_id": "e2",
            "strategy_id": "s1",
            "fill_ts_ms": 2_000,
            "benchmark_ts_ms": 1_995,
            "lag_ms": 5,
            "benchmark_px": pytest.approx(99.5),
            "status": "ok",
            "benchmark_name": "maker_mid",
            "edge_abs": pytest.approx(0.5),
            "edge_bps": pytest.approx(50.0),
        },
    ]


def test_compute_extended_markouts_from_fv_stream_uses_first_row_at_or_after_target() -> None:
    fills = pd.DataFrame(
        [
            {
                "trader_id": "t1",
                "event_id": "e1",
                "strategy_id": "s1",
                "order_side": "BUY",
                "fill_px_num": 100.0,
                "fill_qty_num": 2.0,
                "notional": 200.0,
                "ts_ms": 0,
            },
        ],
    )
    fv_rows = pd.DataFrame(
        [
            {"strategy_id": "s1", "ts_ms": 1_500, "fv": 100.5, "maker_mid": 100.25},
            {"strategy_id": "s1", "ts_ms": 2_500, "fv": 101.5, "maker_mid": 101.25},
        ],
    )

    rows = compute_extended_markouts_from_fv_stream(
        fills=fills,
        fv_rows=fv_rows,
        horizons_s=(1, 2),
        benchmark_names=("fv",),
    )
    records = rows.sort_values("horizon_s").to_dict("records")

    assert records == [
        {
            "trader_id": "t1",
            "event_id": "e1",
            "strategy_id": "s1",
            "target_ts_ms": 1_000,
            "benchmark_ts_ms": 1_500,
            "benchmark_px": pytest.approx(100.5),
            "lag_ms": 500,
            "status": "ok",
            "benchmark_name": "fv",
            "horizon_s": 1,
            "markout_abs": pytest.approx(0.5),
            "markout_bps": pytest.approx(50.0),
        },
        {
            "trader_id": "t1",
            "event_id": "e1",
            "strategy_id": "s1",
            "target_ts_ms": 2_000,
            "benchmark_ts_ms": 2_500,
            "benchmark_px": pytest.approx(101.5),
            "lag_ms": 500,
            "status": "ok",
            "benchmark_name": "fv",
            "horizon_s": 2,
            "markout_abs": pytest.approx(1.5),
            "markout_bps": pytest.approx(150.0),
        },
    ]
