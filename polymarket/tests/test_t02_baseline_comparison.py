"""T02 baseline comparison for the v1 smoke experiment."""

from __future__ import annotations

import csv
import json
import textwrap
from decimal import Decimal
from pathlib import Path

from polymarket.backtest_v1 import run_from_config
from polymarket.models import L2UpdateV1


BASELINE_CSV = Path("polymarket/research/2026-07-01-t02-smoke-pmxt/data/t02_strategy_suite_summary.csv")
EVENT_DIR = Path(
    "polymarket/research/2026-07-01-t02-smoke-pmxt/curated/"
    "t02-pmxt-live-until-1100-no-receive-inversion"
)
STRATEGY_PATH = Path("polymarket/research/2026-07-01-t02-smoke-pmxt/strategy.py")
STRATEGY_CLASSES = {
    "maker_bbo": "MakerBboStrategy",
    "buy_hold_first_ask": "BuyHoldFirstAskStrategy",
    "momentum_taker": "MomentumTakerStrategy",
    "contrarian_taker": "ContrarianTakerStrategy",
}
EXPECTED_V1_STEPS = {"YES": 391, "NO": 390}
TOL = Decimal("1e-9")


def dec(value: str | int | float | None) -> Decimal:
    return Decimal(str(value or "0"))


def read_rows() -> list[dict[str, str]]:
    return list(csv.DictReader(BASELINE_CSV.open(encoding="utf-8-sig")))


def read_fills(path: str) -> list[dict[str, str]]:
    with Path(path).open(encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f))


def write_case_config(tmp_path: Path, row: dict[str, str]) -> Path:
    strategy_class = STRATEGY_CLASSES[row["strategy"]]
    config_path = tmp_path / f"{row['token_side']}-{row['strategy']}.yml"
    config_path.write_text(
        textwrap.dedent(
            f"""
            experiment:
              name: t02_{row['token_side']}_{row['strategy']}_baseline
            adapter:
              name: pmxt_event_v1
              input:
                event_dir: {EVENT_DIR.resolve().as_posix()}
                replay_order: received_time
                drop_until_first_book: true
                asset_id: "{row['token_id']}"
            selection:
              asset_id: "{row['token_id']}"
            strategy:
              path: {STRATEGY_PATH.resolve().as_posix()}
              class: {strategy_class}
              params:
                quantity: "10"
            runtime:
              run_id: {row['token_side']}-{row['strategy']}
            report:
              output_dir: ./runs
            """,
        ).lstrip(),
        encoding="utf-8",
    )
    return config_path


def assert_close(actual: Decimal, expected: Decimal, field: str, case: str) -> None:
    assert abs(actual - expected) <= TOL, f"{case} {field}: {actual} != {expected}"


def summarize_fills(fills: list[dict[str, str]]) -> dict[str, Decimal]:
    buy_qty = sum((dec(fill["quantity"]) for fill in fills if fill["side"] == "BUY"), Decimal("0"))
    sell_qty = sum((dec(fill["quantity"]) for fill in fills if fill["side"] == "SELL"), Decimal("0"))
    gross_notional = sum((dec(fill["price"]) * dec(fill["quantity"]) for fill in fills), Decimal("0"))
    return {"buy_qty": buy_qty, "sell_qty": sell_qty, "gross_notional": gross_notional}


def test_t02_v1_strategy_matrix_matches_pre_reorg_baseline(tmp_path: Path) -> None:
    rows = read_rows()
    assert len(rows) == 8
    assert {(r["token_side"], r["strategy"]) for r in rows} == {
        (side, strategy) for side in {"YES", "NO"} for strategy in STRATEGY_CLASSES
    }

    for row in rows:
        case = f"{row['token_side']} {row['strategy']}"
        summary = run_from_config(write_case_config(tmp_path, row))
        metrics = json.loads(Path(summary["outputs"]["metrics"]).read_text(encoding="utf-8"))
        resolved = json.loads(Path(summary["outputs"]["resolved_config"]).read_text(encoding="utf-8"))
        fills = read_fills(summary["outputs"]["fills_csv"])
        fill_summary = summarize_fills(fills)

        assert int(metrics["steps"]) == EXPECTED_V1_STEPS[row["token_side"]]
        assert int(metrics["steps"]) <= int(row["rows_for_token"])
        assert int(metrics["fills"]) == int(row["fills"]), case
        assert_close(fill_summary["buy_qty"], dec(row["buy_qty"]), "buy_qty", case)
        assert_close(fill_summary["sell_qty"], dec(row["sell_qty"]), "sell_qty", case)
        assert_close(fill_summary["gross_notional"], dec(row["gross_notional"]), "gross_notional", case)
        assert_close(dec(metrics["position"]), dec(row["ending_inventory"]), "ending_inventory", case)
        assert_close(dec(metrics["final_mark_price"]), dec(row["final_mark_price"]), "final_mark_price", case)
        assert_close(dec(metrics["final_equity"]), dec(row["mtm_pnl"]), "mtm_pnl", case)
        assert metrics["final_tick_size"] == row["final_tick_size"]
        assert int(metrics["tick_size_changes_applied"]) == int(row["tick_size_changes_applied"])
        assert row["result_label"] == "smoke_test_unvalidated"
        assert row["replay_order"] == "received_time"
        assert any("PMXT replay_order=received_time" in item for item in resolved["adapter"]["assumptions"])


def test_canonical_updates_do_not_expose_raw_source_payloads() -> None:
    assert "raw" not in L2UpdateV1.__dataclass_fields__
    assert "trade_side" not in L2UpdateV1.__dataclass_fields__
