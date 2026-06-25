#!/usr/bin/env python3
"""Run a small Polymarket Shanghai strategy suite and collect results."""

from __future__ import annotations

import argparse
import csv
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[3]
SCRIPT = Path(__file__).resolve().with_name("run_event_backtest.py")
DEFAULT_OUT_DIR = ROOT / "research" / "2026-06-24-polymarket-shanghai-event-backtest" / "data"
DEFAULT_MANIFEST = ROOT / "research" / "2026-06-24-polymarket-shanghai-event-backtest" / "suite_manifest.json"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--curated-root", type=Path, default=None)
    parser.add_argument("--out-dir", type=Path, default=DEFAULT_OUT_DIR)
    parser.add_argument("--quote-size", type=float, default=10.0)
    parser.add_argument("--max-inventory", type=float, default=100.0)
    parser.add_argument("--decision-frequency", default="5min")
    parser.add_argument("--signal-threshold", type=float, default=0.03)
    return parser.parse_args()


def load_manifest(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def repo_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


def run_case(args: argparse.Namespace, event_slug: str, market_label: str, token_side: str, strategy: str) -> dict[str, Any]:
    command = [
        sys.executable,
        str(SCRIPT),
        "--event-slug",
        event_slug,
        "--market-label",
        market_label,
        "--token-side",
        token_side,
        "--strategy",
        strategy,
        "--out-dir",
        str(args.out_dir),
        "--quote-size",
        str(args.quote_size),
        "--max-inventory",
        str(args.max_inventory),
        "--decision-frequency",
        args.decision_frequency,
        "--signal-threshold",
        str(args.signal_threshold),
    ]
    if args.curated_root is not None:
        command.extend(["--curated-root", str(args.curated_root)])
    completed = subprocess.run(command, check=True, text=True, capture_output=True)
    return json.loads(completed.stdout)


def main() -> None:
    args = parse_args()
    manifest = load_manifest(args.manifest)
    defaults = manifest.get("defaults", {})
    if "--quote-size" not in sys.argv:
        args.quote_size = float(defaults.get("quote_size", args.quote_size))
    if "--max-inventory" not in sys.argv:
        args.max_inventory = float(defaults.get("max_inventory", args.max_inventory))
    if "--decision-frequency" not in sys.argv:
        args.decision_frequency = str(defaults.get("decision_frequency", args.decision_frequency))
    if "--signal-threshold" not in sys.argv:
        args.signal_threshold = float(defaults.get("signal_threshold", args.signal_threshold))

    args.out_dir.mkdir(parents=True, exist_ok=True)
    summaries: list[dict[str, Any]] = []

    cases = manifest["cases"]
    strategies = manifest["strategies"]
    for case in cases:
        event_slug = case["event_slug"]
        market_label = case["market_label"]
        token_side = case["token_side"]
        for strategy in strategies:
            summary = run_case(args, event_slug, market_label, token_side, strategy)
            summaries.append(summary)
            print(
                f"{event_slug} {market_label} {strategy}: "
                f"fills={summary['backtest']['fills']} "
                f"settlement_pnl={summary['backtest']['settlement_pnl']}"
            )

    suite_path = args.out_dir / "strategy_suite_summary.csv"
    with suite_path.open("w", newline="", encoding="utf-8") as f:
        fieldnames = [
            "event_slug",
            "market_label",
            "token_side",
            "strategy",
            "rows_for_token",
            "fills",
            "buy_qty",
            "sell_qty",
            "ending_inventory",
            "gross_notional",
            "settlement_pnl",
            "return_on_gross_notional",
            "snapshot_pairs",
            "snapshot_bbo_mismatches",
            "snapshot_bbo_mismatch_rate",
            "trades_checked",
            "trades_off_book",
            "trade_off_book_rate",
            "trade_side_touch_rate",
            "summary_json",
            "result_label",
            "results_validated",
        ]
        writer = csv.DictWriter(f, fieldnames=fieldnames)
        writer.writeheader()
        for summary in summaries:
            row = {
                "event_slug": summary["selection"]["event_slug"],
                "market_label": summary["selection"]["market_label"],
                "token_side": summary["selection"]["token_side"],
                "strategy": summary["inputs"]["strategy"],
                "rows_for_token": summary["inputs"]["rows_for_token"],
                "fills": summary["backtest"]["fills"],
                "buy_qty": summary["backtest"]["buy_qty"],
                "sell_qty": summary["backtest"]["sell_qty"],
                "ending_inventory": summary["backtest"]["ending_inventory"],
                "gross_notional": summary["backtest"]["gross_notional"],
                "settlement_pnl": summary["backtest"]["settlement_pnl"],
                "return_on_gross_notional": summary["backtest"]["return_on_gross_notional"],
                "snapshot_pairs": summary["replay_quality"]["snapshot_alignment"]["snapshot_pairs"],
                "snapshot_bbo_mismatches": summary["replay_quality"]["snapshot_alignment"][
                    "snapshot_bbo_mismatches"
                ],
                "snapshot_bbo_mismatch_rate": summary["replay_quality"]["snapshot_alignment"][
                    "snapshot_bbo_mismatch_rate"
                ],
                "trades_checked": summary["replay_quality"]["trade_sanity"]["trades_checked"],
                "trades_off_book": summary["replay_quality"]["trade_sanity"]["trades_off_book"],
                "trade_off_book_rate": summary["replay_quality"]["trade_sanity"]["trade_off_book_rate"],
                "trade_side_touch_rate": summary["replay_quality"]["trade_sanity"]["trade_side_touch_rate"],
                "summary_json": summary["outputs"]["summary_json"],
                "result_label": summary["replay_quality"]["result_label"],
                "results_validated": summary["replay_quality"]["results_validated"],
            }
            writer.writerow(row)

    print(f"wrote {repo_path(suite_path)}")


if __name__ == "__main__":
    main()
