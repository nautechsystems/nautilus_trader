#!/usr/bin/env python3
"""Check local data health before relying on Polymarket replay/backtest tests.

This script intentionally does not download anything. It inspects already-local
artifacts only:
- PolyReaper T02 PMXT/raw alignment diagnostics, if present;
- PMXT hourly parquet metadata, if present;
- the repo-local T02 smoke backtest summary generated in the previous step.
"""

from __future__ import annotations

import csv
import json
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import pyarrow.parquet as pq

ROOT = Path(__file__).resolve().parents[3]
OUT_DIR = ROOT / "research" / "2026-07-01-polymarket-backtest-correctness"
DATA_DIR = OUT_DIR / "data"
REPORT_PATH = OUT_DIR / "data_health_report.md"
JSON_PATH = DATA_DIR / "data_health_summary.json"

DEFAULT_POLYREAPER_ROOT = Path(os.environ.get("POLYREAPER_ROOT", "C:/Projects/PolyReaper"))

PMXT_HOURLY = Path(
    os.environ.get(
        "POLYMARKET_T02_PMXT_HOURLY",
        str(
            DEFAULT_POLYREAPER_ROOT
            / "data/external/pmxt/polymarket/v2/orderbook/hourly/"
            / "polymarket_orderbook_2026-06-26T02.parquet"
        ),
    )
)
ALIGNMENT_DIAG = Path(
    os.environ.get(
        "POLYMARKET_T02_ALIGNMENT_DIAG",
        str(
            DEFAULT_POLYREAPER_ROOT
            / "research/2026-06-25-polymarket-raw-ws-ordering-capture/"
            / "data/diagnostics/live_until_1100_pmxt_alignment_diagnostics.json"
        ),
    )
)
RAW_ORDERING_DIAG = Path(
    os.environ.get(
        "POLYMARKET_T02_RAW_ORDERING_DIAG",
        str(
            DEFAULT_POLYREAPER_ROOT
            / "research/2026-06-25-polymarket-raw-ws-ordering-capture/"
            / "data/diagnostics/live_until_1100_raw_ws_ordering_diagnostics.json"
        ),
    )
)
T02_SMOKE_SUMMARY = (
    ROOT
    / "research"
    / "2026-07-01-polymarket-t02-no-ordering-backtest"
    / "data"
    / "t02_strategy_suite_summary.csv"
)


def repo_or_abs(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        return str(path)


def read_json(path: Path) -> dict[str, Any] | None:
    if not path.exists():
        return None
    return json.loads(path.read_text(encoding="utf-8"))


def parquet_metadata(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {"exists": False, "path": str(path)}
    pf = pq.ParquetFile(path)
    return {
        "exists": True,
        "path": str(path),
        "bytes": path.stat().st_size,
        "rows": pf.metadata.num_rows,
        "row_groups": pf.num_row_groups,
        "columns": pf.schema.names,
    }


def summarize_alignment(diag: dict[str, Any] | None) -> dict[str, Any]:
    if diag is None:
        return {"exists": False}
    match = diag.get("signature_match", {})
    pmxt_summary = diag.get("pmxt_summary", {})
    receive_orders = pmxt_summary.get("timestamp_received_order", {})
    source_orders = pmxt_summary.get("timestamp_order", {})
    receive_inversions = sum(int(v.get("inversions", 0) or 0) for v in receive_orders.values())
    source_inversions = sum(int(v.get("inversions", 0) or 0) for v in source_orders.values())
    return {
        "exists": True,
        "status": diag.get("status"),
        "raw_window_utc": diag.get("raw_window_utc"),
        "pmxt_row_count": pmxt_summary.get("row_count"),
        "pmxt_event_type_counts": pmxt_summary.get("event_type_counts"),
        "raw_signature_count": match.get("raw_signature_count"),
        "pmxt_signature_count": match.get("pmxt_signature_count"),
        "matched_signature_count": match.get("matched_signature_count"),
        "raw_minus_pmxt_count": match.get("raw_minus_pmxt_count"),
        "pmxt_minus_raw_count": match.get("pmxt_minus_raw_count"),
        "pmxt_received_time_inversions": receive_inversions,
        "pmxt_source_time_inversions": source_inversions,
        "passes_alignment_gate": bool(
            match.get("pmxt_signature_count")
            and match.get("matched_signature_count") == match.get("pmxt_signature_count")
            and match.get("pmxt_minus_raw_count") == 0
            and receive_inversions == 0
        ),
    }


def summarize_raw_ordering(diag: dict[str, Any] | None) -> dict[str, Any]:
    if diag is None:
        return {"exists": False}
    receive = diag.get("receive_order", {})
    timestamp = diag.get("timestamp_order", {})
    return {
        "exists": True,
        "row_count": diag.get("row_count"),
        "parse_error_count": diag.get("parse_error_count"),
        "local_index_inversions": receive.get("local_index_inversions"),
        "recv_monotonic_ns_inversions": receive.get("recv_monotonic_ns_inversions"),
        "recv_wall_time_inversions": receive.get("recv_wall_time_inversions"),
        "total_asset_series_inversions": timestamp.get("total_asset_series_inversions"),
        "total_event_type_series_inversions": timestamp.get("total_event_type_series_inversions"),
        "passes_receive_order_gate": (
            diag.get("parse_error_count") == 0
            and receive.get("local_index_inversions") == 0
            and receive.get("recv_monotonic_ns_inversions") == 0
            and receive.get("recv_wall_time_inversions") == 0
            and timestamp.get("total_asset_series_inversions") == 0
        ),
    }


def summarize_t02_smoke(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {"exists": False, "path": repo_or_abs(path)}
    rows = list(csv.DictReader(path.open(encoding="utf-8")))
    return {
        "exists": True,
        "path": repo_or_abs(path),
        "rows": len(rows),
        "replay_orders": sorted({row["replay_order"] for row in rows}),
        "result_labels": sorted({row["result_label"] for row in rows}),
        "max_batch_mismatch_rate": max(float(row["price_change_batch_mismatch_rate"] or 0) for row in rows),
        "max_fill_tick_violations": max(int(row["fill_tick_price_violations"] or 0) for row in rows),
        "total_fills": sum(int(row["fills"] or 0) for row in rows),
        "passes_smoke_gate": (
            len(rows) == 8
            and {row["replay_order"] for row in rows} == {"received_time"}
            and all(float(row["price_change_batch_mismatch_rate"] or 0) == 0.0 for row in rows)
            and all(int(row["fill_tick_price_violations"] or 0) == 0 for row in rows)
        ),
    }


def write_report(summary: dict[str, Any]) -> None:
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    JSON_PATH.write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")

    pmxt = summary["pmxt_hourly"]
    align = summary["alignment_diag"]
    raw = summary["raw_ordering_diag"]
    smoke = summary["t02_smoke"]
    overall = summary["overall_pass"]
    lines = [
        "# Polymarket T02 数据健康检查",
        "",
        f"生成时间：`{summary['generated_at']}`",
        "",
        "## 结论",
        "",
        (
            "本地 T02 数据健康门通过，可继续用作 received-time replay smoke / synthetic golden tests 的背景样本。"
            if overall
            else "本地 T02 数据健康门未完全通过；见下方缺失或失败项。"
        ),
        "",
        "注意：这个报告只检查已落地数据，不证明策略收益，也不替代 synthetic golden tests。",
        "",
        "## PMXT 小时包 metadata",
        "",
        f"- exists: `{pmxt.get('exists')}`",
        f"- path: `{pmxt.get('path')}`",
        f"- rows: `{pmxt.get('rows')}`",
        f"- row_groups: `{pmxt.get('row_groups')}`",
        f"- bytes: `{pmxt.get('bytes')}`",
        f"- columns: `{', '.join(pmxt.get('columns', [])) if pmxt.get('columns') else ''}`",
        "",
        "路径可通过环境变量覆盖：`POLYREAPER_ROOT`、`POLYMARKET_T02_PMXT_HOURLY`、"
        "`POLYMARKET_T02_ALIGNMENT_DIAG`、`POLYMARKET_T02_RAW_ORDERING_DIAG`。",
        "",
        "## PMXT/raw 对齐诊断",
        "",
        f"- exists: `{align.get('exists')}`",
        f"- status: `{align.get('status')}`",
        f"- raw_window_utc: `{align.get('raw_window_utc')}`",
        f"- PMXT signatures: `{align.get('pmxt_signature_count')}`",
        f"- matched signatures: `{align.get('matched_signature_count')}`",
        f"- PMXT - raw: `{align.get('pmxt_minus_raw_count')}`",
        f"- PMXT timestamp_received inversions: `{align.get('pmxt_received_time_inversions')}`",
        f"- PMXT source timestamp inversions: `{align.get('pmxt_source_time_inversions')}`",
        f"- gate: `{align.get('passes_alignment_gate')}`",
        "",
        "## raw receive ordering 诊断",
        "",
        f"- exists: `{raw.get('exists')}`",
        f"- parse_error_count: `{raw.get('parse_error_count')}`",
        f"- local_index_inversions: `{raw.get('local_index_inversions')}`",
        f"- recv_monotonic_ns_inversions: `{raw.get('recv_monotonic_ns_inversions')}`",
        f"- recv_wall_time_inversions: `{raw.get('recv_wall_time_inversions')}`",
        f"- total_asset_series_inversions: `{raw.get('total_asset_series_inversions')}`",
        f"- total_event_type_series_inversions: `{raw.get('total_event_type_series_inversions')}`",
        f"- gate: `{raw.get('passes_receive_order_gate')}`",
        "",
        "## 本仓库 T02 smoke 输出",
        "",
        f"- exists: `{smoke.get('exists')}`",
        f"- path: `{smoke.get('path')}`",
        f"- rows: `{smoke.get('rows')}`",
        f"- replay_orders: `{smoke.get('replay_orders')}`",
        f"- result_labels: `{smoke.get('result_labels')}`",
        f"- max_batch_mismatch_rate: `{smoke.get('max_batch_mismatch_rate')}`",
        f"- max_fill_tick_violations: `{smoke.get('max_fill_tick_violations')}`",
        f"- total_fills: `{smoke.get('total_fills')}`",
        f"- gate: `{smoke.get('passes_smoke_gate')}`",
        "",
        "## 对测试设计的含义",
        "",
        "- T02 真实样本可作为 received-time replay 的背景 sanity check。",
        "- PMXT source timestamp 仍有倒序，因此不能用它证明 source-time replay 正确。",
        "- 引擎正确性仍必须靠 synthetic golden tests：手工构造盘口、batch、tick、trade、PnL，并和手算结果比对。",
    ]
    REPORT_PATH.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    summary = {
        "generated_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "pmxt_hourly": parquet_metadata(PMXT_HOURLY),
        "alignment_diag": summarize_alignment(read_json(ALIGNMENT_DIAG)),
        "raw_ordering_diag": summarize_raw_ordering(read_json(RAW_ORDERING_DIAG)),
        "t02_smoke": summarize_t02_smoke(T02_SMOKE_SUMMARY),
    }
    summary["overall_pass"] = bool(
        summary["pmxt_hourly"].get("exists")
        and summary["alignment_diag"].get("passes_alignment_gate")
        and summary["raw_ordering_diag"].get("passes_receive_order_gate")
        and summary["t02_smoke"].get("passes_smoke_gate")
    )
    write_report(summary)
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    if not summary["overall_pass"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
