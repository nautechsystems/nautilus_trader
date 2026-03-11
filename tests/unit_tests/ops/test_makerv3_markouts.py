from __future__ import annotations

from decimal import Decimal

import pytest

from ops.scripts.makerv3_markouts import compute_markout_rows
from ops.scripts.makerv3_markouts import load_stream_rows
from ops.scripts.makerv3_markouts import summarize_markout_rows


def test_compute_markout_rows_uses_first_fv_at_or_after_each_horizon() -> None:
    trade_rows = [
        {
            "strategy_id": "plumeusdt_bybit_perp_makerv3",
            "trade_id": "trade-1",
            "side": "BUY",
            "price": "100",
            "qty": "2",
            "ts_ms": 1_000,
        },
        {
            "strategy_id": "plumeusdt_bybit_perp_makerv3",
            "trade_id": "trade-2",
            "side": "SELL",
            "price": "100",
            "qty": "2",
            "ts_ms": 2_000,
        },
    ]
    fv_rows = [
        {"strategy_id": "plumeusdt_bybit_perp_makerv3", "fv": "101", "ts_ms": 31_000},
        {"strategy_id": "plumeusdt_bybit_perp_makerv3", "fv": "103", "ts_ms": 61_500},
        {"strategy_id": "plumeusdt_bybit_perp_makerv3", "fv": "104", "ts_ms": 121_250},
        {"strategy_id": "plumeusdt_bybit_perp_makerv3", "fv": "99", "ts_ms": 32_000},
        {"strategy_id": "plumeusdt_bybit_perp_makerv3", "fv": "98", "ts_ms": 62_500},
        {"strategy_id": "plumeusdt_bybit_perp_makerv3", "fv": "97", "ts_ms": 122_250},
    ]

    rows = compute_markout_rows(trade_rows=trade_rows, fv_rows=fv_rows, horizons_s=(30, 60, 120))

    grouped = {(row["trade_id"], row["horizon_s"]): row for row in rows}
    assert grouped[("trade-1", 30)]["markout_abs"] == Decimal("1")
    assert grouped[("trade-1", 60)]["markout_abs"] == Decimal("3")
    assert grouped[("trade-1", 120)]["markout_abs"] == Decimal("4")
    assert grouped[("trade-2", 30)]["markout_abs"] == Decimal("1")
    assert grouped[("trade-2", 60)]["markout_abs"] == Decimal("2")
    assert grouped[("trade-2", 120)]["markout_abs"] == Decimal("3")


def test_compute_markout_rows_marks_missing_future_fv() -> None:
    rows = compute_markout_rows(
        trade_rows=[
            {
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "trade_id": "trade-3",
                "side": "BUY",
                "price": "100",
                "qty": "1",
                "ts_ms": 1_000,
            },
        ],
        fv_rows=[],
        horizons_s=(30,),
    )

    assert rows == [
        {
            "strategy_id": "plumeusdt_bybit_perp_makerv3",
            "trade_id": "trade-3",
            "fill_id": "trade-3",
            "horizon_s": 30,
            "fill_side": "BUY",
            "fill_px": Decimal("100"),
            "fill_qty": Decimal("1"),
            "fill_ts_ms": 1_000,
            "benchmark_px": None,
            "benchmark_ts_ms": None,
            "markout_abs": None,
            "markout_bps": None,
            "status": "missing_future_fv",
        },
    ]


def test_compute_markout_rows_marks_truncated_fv_window_as_missing_future_fv() -> None:
    rows = compute_markout_rows(
        trade_rows=[
            {
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "trade_id": "trade-4",
                "side": "BUY",
                "price": "100",
                "qty": "1",
                "ts_ms": 1_000,
            },
        ],
        fv_rows=[
            {
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "fv": "102",
                "ts_ms": 61_000,
            },
        ],
        horizons_s=(30,),
    )

    assert rows == [
        {
            "strategy_id": "plumeusdt_bybit_perp_makerv3",
            "trade_id": "trade-4",
            "fill_id": "trade-4",
            "horizon_s": 30,
            "fill_side": "BUY",
            "fill_px": Decimal("100"),
            "fill_qty": Decimal("1"),
            "fill_ts_ms": 1_000,
            "benchmark_px": None,
            "benchmark_ts_ms": None,
            "markout_abs": None,
            "markout_bps": None,
            "status": "missing_future_fv",
        },
    ]


def test_summarize_markout_rows_groups_by_horizon() -> None:
    summary = summarize_markout_rows(
        [
            {
                "strategy_id": "s1",
                "horizon_s": 30,
                "markout_abs": Decimal("1"),
                "markout_bps": Decimal("100"),
                "status": "ok",
            },
            {
                "strategy_id": "s1",
                "horizon_s": 30,
                "markout_abs": Decimal("-0.5"),
                "markout_bps": Decimal("-50"),
                "status": "ok",
            },
            {
                "strategy_id": "s1",
                "horizon_s": 60,
                "markout_abs": None,
                "markout_bps": None,
                "status": "missing_future_fv",
            },
        ],
    )

    assert summary == [
        {
            "horizon_s": 30,
            "count": 2,
            "avg_markout_abs": Decimal("0.25"),
            "avg_markout_bps": Decimal("25"),
        },
        {
            "horizon_s": 60,
            "count": 0,
            "avg_markout_abs": None,
            "avg_markout_bps": None,
        },
    ]


def test_load_stream_rows_supports_the_planned_two_argument_helper_contract() -> None:
    class FakeRedis:
        def __init__(self) -> None:
            self.calls: list[tuple[str, int | None]] = []

        def xrevrange(
            self,
            key: str,
            max: str = "+",
            min: str = "-",
            count: int | None = None,
        ) -> list[tuple[str, dict[str, str]]]:
            self.calls.append((key, count))
            return []

    redis_client = FakeRedis()

    trade_rows, fv_rows = load_stream_rows(redis_client, "plumeusdt_bybit_perp_makerv3")

    assert trade_rows == []
    assert fv_rows == []
    assert len(redis_client.calls) == 2


def test_compute_markout_rows_exposes_stable_fill_identity_when_trade_ids_collide() -> None:
    rows = compute_markout_rows(
        trade_rows=[
            {
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "trade_id": "shared-trade-id",
                "row_id": "row-1",
                "side": "BUY",
                "price": "100",
                "qty": "1",
                "ts_ms": 1_000,
            },
            {
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "trade_id": "shared-trade-id",
                "row_id": "row-2",
                "side": "BUY",
                "price": "100",
                "qty": "1",
                "ts_ms": 2_000,
            },
        ],
        fv_rows=[
            {
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "fv": "101",
                "ts_ms": 32_000,
            },
        ],
        horizons_s=(30,),
    )

    assert [row["trade_id"] for row in rows] == ["shared-trade-id", "shared-trade-id"]
    assert [row["fill_id"] for row in rows] == ["row-1", "row-2"]


def test_load_stream_rows_pages_fv_until_oldest_trade_target_is_covered(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    import flux.api._payloads_common as payloads_common

    def fake_extract_stream_rows(stream_entries):
        return [fields["row"] for _, fields in stream_entries]

    monkeypatch.setattr(payloads_common, "extract_stream_rows", fake_extract_stream_rows)

    trade_entries = [
        (
            "1000-0",
            {
                "row": {
                    "strategy_id": "plumeusdt_bybit_perp_makerv3",
                    "trade_id": "trade-1",
                    "side": "BUY",
                    "price": "100",
                    "qty": "1",
                    "ts_ms": 1_000,
                },
            },
        ),
    ]
    fv_pages = {
        "+": [
            (
                "61000-0",
                {
                    "row": {
                        "strategy_id": "plumeusdt_bybit_perp_makerv3",
                        "fv": "102",
                        "ts_ms": 61_000,
                    },
                },
            ),
            (
                "60000-0",
                {
                    "row": {
                        "strategy_id": "plumeusdt_bybit_perp_makerv3",
                        "fv": "102",
                        "ts_ms": 60_000,
                    },
                },
            ),
        ],
        "(60000-0": [
            (
                "31000-0",
                {
                    "row": {
                        "strategy_id": "plumeusdt_bybit_perp_makerv3",
                        "fv": "101",
                        "ts_ms": 31_000,
                    },
                },
            ),
        ],
    }

    class FakeRedis:
        def __init__(self) -> None:
            self.fv_max_args: list[str] = []

        def xrevrange(
            self,
            key: str,
            max: str = "+",
            min: str = "-",
            count: int | None = None,
        ):
            if ":trades:stream:" in key:
                return list(reversed(trade_entries))
            if ":fv:stream:" in key:
                self.fv_max_args.append(max)
                return fv_pages.get(max, [])
            raise AssertionError(f"unexpected key {key}")

    redis_client = FakeRedis()

    trade_rows, fv_rows = load_stream_rows(
        redis_client,
        "plumeusdt_bybit_perp_makerv3",
        limit=1,
        horizons_s=(30,),
    )

    rows = compute_markout_rows(trade_rows=trade_rows, fv_rows=fv_rows, horizons_s=(30,))

    assert redis_client.fv_max_args == ["+", "(60000-0"]
    assert rows[0]["benchmark_ts_ms"] == 31_000
    assert rows[0]["markout_abs"] == Decimal("1")
