from __future__ import annotations

import argparse
import csv
import importlib.util
import json
import sys
from pathlib import Path
from typing import Any

import pandas as pd


SCRIPT_PATH = (
    Path(__file__).resolve().parents[1]
    / "scripts"
    / "run_event_backtest.py"
)
spec = importlib.util.spec_from_file_location("polymarket_run_event_backtest", SCRIPT_PATH)
assert spec is not None and spec.loader is not None
harness = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = harness
spec.loader.exec_module(harness)

TOKEN_ID = "11111111111111111111111111111111111111111111111111111111111111111"
NO_TOKEN_ID = "22222222222222222222222222222222222222222222222222222222222222222"
MARKET = "0xsynthetic"
EVENT_SLUG = "synthetic-golden-event"
MARKET_LABEL = "Synthetic binary market"


def ts(seconds: int, millis: int = 0) -> pd.Timestamp:
    return pd.Timestamp("2026-01-01T00:00:00Z") + pd.Timedelta(seconds=seconds, milliseconds=millis)


def levels(*pairs: tuple[float, float]) -> str:
    return json.dumps([[price, size] for price, size in pairs])


def row(
    *,
    event_type: str,
    received: pd.Timestamp,
    source: pd.Timestamp | None = None,
    asset_id: str = TOKEN_ID,
    bids: str | None = None,
    asks: str | None = None,
    price: float | None = None,
    size: float | None = None,
    side: str | None = None,
    best_bid: float | None = None,
    best_ask: float | None = None,
    tx: str | None = None,
    old_tick_size: str | None = None,
    new_tick_size: str | None = None,
) -> dict[str, Any]:
    return {
        "timestamp_received": received,
        "timestamp": source if source is not None else received,
        "market": MARKET,
        "event_type": event_type,
        "asset_id": asset_id,
        "bids": bids,
        "asks": asks,
        "price": price,
        "size": size,
        "side": side,
        "best_bid": best_bid,
        "best_ask": best_ask,
        "fee_rate_bps": None,
        "transaction_hash": tx,
        "old_tick_size": old_tick_size,
        "new_tick_size": new_tick_size,
    }


def write_case(tmp_path: Path, rows: list[dict[str, Any]], *, settlement: float | None = None) -> Path:
    event_dir = tmp_path / EVENT_SLUG
    event_dir.mkdir(parents=True)
    (event_dir / "event_index.json").write_text(
        json.dumps(
            {
                "eventSlug": EVENT_SLUG,
                "title": "Synthetic golden event",
                "markets": [
                    {
                        "label": MARKET_LABEL,
                        "marketId": "synthetic-market-1",
                        "conditionId": MARKET,
                        "question": "Synthetic binary market?",
                        "yesToken": TOKEN_ID,
                        "noToken": NO_TOKEN_ID,
                    }
                ],
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )
    markets = []
    if settlement is not None:
        markets = [
            {
                "id": "synthetic-market-1",
                "outcomes": ["YES", "NO"],
                "outcomePrices": [settlement, 1.0 - settlement],
            }
        ]
    (event_dir / "gamma_event.raw.json").write_text(
        json.dumps({"markets": markets}, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    pd.DataFrame(rows).to_parquet(event_dir / "orderbook.parquet", index=False)
    return tmp_path


def run_case(
    tmp_path: Path,
    rows: list[dict[str, Any]],
    *,
    strategy: str,
    replay_order: str = "received_time",
    decision_frequency: str = "0ms",
    signal_threshold: float = 0.03,
    quote_size: float = 10.0,
    max_inventory: float = 100.0,
) -> dict[str, Any]:
    curated_root = write_case(tmp_path / "curated", rows)
    args = argparse.Namespace(
        event_slug=EVENT_SLUG,
        market_label=MARKET_LABEL,
        token_side="YES",
        curated_root=curated_root,
        out_dir=tmp_path / "out" / strategy / replay_order,
        strategy=strategy,
        quote_size=quote_size,
        max_inventory=max_inventory,
        decision_frequency=decision_frequency,
        signal_threshold=signal_threshold,
        fill_model="conservative",
        timeseries_frequency="1ms",
        replay_order=replay_order,
    )
    return harness.run_backtest(args)


def read_fills(summary: dict[str, Any]) -> list[dict[str, str]]:
    path = Path(summary["outputs"]["fills_csv"])
    return list(csv.DictReader(path.open(encoding="utf-8")))


def assert_close(actual: float | None, expected: float, *, eps: float = 1e-9) -> None:
    assert actual is not None
    assert abs(actual - expected) <= eps


def test_momentum_strategy_cannot_trade_before_future_price_jump(tmp_path: Path) -> None:
    rows = [
        row(
            event_type="book",
            received=ts(0),
            bids=levels((0.50, 100.0)),
            asks=levels((0.52, 100.0)),
        ),
        row(event_type="price_change", received=ts(1), price=0.56, size=100.0, side="BUY", best_bid=0.56, best_ask=0.58),
        row(event_type="price_change", received=ts(1), price=0.52, size=0.0, side="SELL", best_bid=0.56, best_ask=0.58),
        row(event_type="price_change", received=ts(1), price=0.58, size=100.0, side="SELL", best_bid=0.56, best_ask=0.58),
    ]

    summary = run_case(tmp_path, rows, strategy="momentum_taker", signal_threshold=0.03)
    fills = read_fills(summary)

    assert summary["backtest"]["fills"] == 1
    assert fills[0]["timestamp"] == "2026-01-01T00:00:01Z"
    assert fills[0]["fill_side"] == "BUY"
    assert_close(float(fills[0]["price"]), 0.58)
    assert_close(summary["backtest"]["ending_inventory"], 10.0)
    assert_close(summary["backtest"]["ending_cash"], -5.8)


def test_price_change_batch_is_atomic_for_strategy_decisions(tmp_path: Path) -> None:
    rows = [
        row(
            event_type="book",
            received=ts(0),
            bids=levels((0.50, 100.0)),
            asks=levels((0.60, 100.0)),
        ),
        # Same PMXT batch key. A row-by-row engine would see a transient 0.58 bid
        # and a 0.59 mid, which crosses the 0.03 momentum threshold. Atomic batch
        # replay applies both rows before strategy decisions, so final BBO is unchanged.
        row(event_type="price_change", received=ts(1), price=0.58, size=100.0, side="BUY", best_bid=0.50, best_ask=0.60),
        row(event_type="price_change", received=ts(1), price=0.58, size=0.0, side="BUY", best_bid=0.50, best_ask=0.60),
    ]

    summary = run_case(tmp_path, rows, strategy="momentum_taker", signal_threshold=0.03)

    assert summary["backtest"]["fills"] == 0
    assert summary["replay_quality"]["pmxt_derived_bbo_diagnostic"]["price_change_batch_compared"] == 1
    assert summary["replay_quality"]["pmxt_derived_bbo_diagnostic"]["price_change_batch_mismatches"] == 0
    assert summary["replay_quality"]["pmxt_derived_bbo_diagnostic"]["split_price_change_batch_key_count"] == 0
    assert_close(summary["backtest"]["final_best_bid"], 0.50)
    assert_close(summary["backtest"]["final_best_ask"], 0.60)


def test_replay_order_is_explicit_when_source_timestamps_invert(tmp_path: Path) -> None:
    rows = [
        row(
            event_type="book",
            received=ts(0),
            source=ts(0),
            bids=levels((0.50, 100.0)),
            asks=levels((0.70, 100.0)),
        ),
        row(
            event_type="price_change",
            received=ts(1),
            source=ts(2),
            price=0.60,
            size=100.0,
            side="BUY",
            best_bid=0.60,
            best_ask=0.70,
        ),
        row(
            event_type="price_change",
            received=ts(2),
            source=ts(1),
            price=0.60,
            size=0.0,
            side="BUY",
            best_bid=0.50,
            best_ask=0.70,
        ),
    ]

    received_summary = run_case(tmp_path / "received", rows, strategy="maker_bbo", replay_order="received_time")
    source_summary = run_case(tmp_path / "source", rows, strategy="maker_bbo", replay_order="source_time")

    assert received_summary["inputs"]["replay_order"] == "received_time"
    assert source_summary["inputs"]["replay_order"] == "source_time"
    assert_close(received_summary["backtest"]["final_best_bid"], 0.50)
    assert_close(source_summary["backtest"]["final_best_bid"], 0.60)


def test_tick_size_change_is_causal_for_fill_price_validation(tmp_path: Path) -> None:
    rows = [
        row(
            event_type="book",
            received=ts(0),
            bids=levels((0.95, 100.0)),
            asks=levels((0.965, 100.0)),
        ),
        row(event_type="last_trade_price", received=ts(1), price=0.965, size=10.0, side="BUY", tx="before_tick"),
        row(event_type="tick_size_change", received=ts(2), old_tick_size="0.01", new_tick_size="0.001"),
        row(event_type="last_trade_price", received=ts(3), price=0.965, size=10.0, side="BUY", tx="after_tick"),
    ]

    summary = run_case(tmp_path, rows, strategy="maker_bbo")
    tick = summary["replay_quality"]["tick_size"]

    assert summary["backtest"]["fills"] == 2
    assert tick["tick_size_changes_applied"] == 1
    assert tick["old_tick_size_mismatches"] == 0
    assert tick["final_tick_size"] == "0.001"
    assert tick["fill_tick_price_checks"] == 2
    assert tick["fill_tick_price_violations"] == 1
    assert tick["fill_tick_price_violation_examples"][0]["timestamp"] == "2026-01-01T00:00:01Z"


def test_maker_fill_partial_trade_size_and_mtm_pnl_are_hand_calculated(tmp_path: Path) -> None:
    rows = [
        row(
            event_type="book",
            received=ts(0),
            bids=levels((0.50, 100.0)),
            asks=levels((0.60, 100.0)),
        ),
        row(event_type="last_trade_price", received=ts(1), price=0.50, size=7.0, side="SELL", tx="sell_into_bid"),
        row(event_type="price_change", received=ts(2), price=0.55, size=100.0, side="BUY", best_bid=0.55, best_ask=0.65),
        row(event_type="price_change", received=ts(2), price=0.60, size=0.0, side="SELL", best_bid=0.55, best_ask=0.65),
        row(event_type="price_change", received=ts(2), price=0.65, size=100.0, side="SELL", best_bid=0.55, best_ask=0.65),
    ]

    summary = run_case(tmp_path, rows, strategy="maker_bbo")
    fills = read_fills(summary)

    assert summary["backtest"]["fills"] == 1
    assert fills[0]["fill_side"] == "BUY"
    assert_close(float(fills[0]["quantity"]), 7.0)
    assert_close(float(fills[0]["price"]), 0.50)
    assert_close(summary["backtest"]["ending_inventory"], 7.0)
    assert_close(summary["backtest"]["ending_cash"], -3.50)
    assert_close(summary["backtest"]["final_best_bid"], 0.55)
    assert_close(summary["backtest"]["final_best_ask"], 0.65)
    assert_close(summary["backtest"]["final_mark_price"], 0.60)
    assert_close(summary["backtest"]["mtm_pnl"], 0.70)
