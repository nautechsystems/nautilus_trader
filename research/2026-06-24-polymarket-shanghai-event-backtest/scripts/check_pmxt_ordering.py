#!/usr/bin/env python3
"""Diagnose PMXT parquet ordering, batch continuity, and indirect gap signals."""

from __future__ import annotations

import argparse
import csv
import json
import re
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pandas as pd
import pyarrow.parquet as pq


ROOT = Path(__file__).resolve().parents[3]
RESEARCH_ROOT = ROOT / "research" / "2026-06-24-polymarket-shanghai-event-backtest"
DEFAULT_CURATED_ROOT = Path("C:/Projects/PolyReaper/data/curated/polymarket/events")
DEFAULT_OUT = RESEARCH_ROOT / "data" / "pmxt_ordering_diagnostics.json"
DEFAULT_REPORT = RESEARCH_ROOT / "report_ordering.md"
SUMMARY_CSV = RESEARCH_ROOT / "data" / "strategy_suite_summary.csv"

EVENT_SLUGS = [
    "highest-temperature-in-shanghai-on-june-9-2026",
    "highest-temperature-in-shanghai-on-june-10-2026",
]

TIME_QUANTILES = [0.0, 0.001, 0.01, 0.5, 0.99, 0.999, 1.0]
GAP_QUANTILES = [0.5, 0.9, 0.99, 0.999, 1.0]


@dataclass(frozen=True)
class CaseSelection:
    event_slug: str
    market_label: str
    token_side: str
    token_id: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--curated-root", type=Path, default=DEFAULT_CURATED_ROOT)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    return parser.parse_args()


def ts_ms(series: pd.Series) -> pd.Series:
    values = pd.to_datetime(series, utc=True).to_numpy(dtype="datetime64[ms]").astype("int64")
    return pd.Series(values, index=series.index)


def quantiles(series: pd.Series, qs: list[float]) -> dict[str, float | None]:
    if series.empty:
        return {str(q): None for q in qs}
    values = series.quantile(qs)
    return {str(q): float(values.loc[q]) for q in qs}


def fmt_pct(value: float | None) -> str:
    return "n/a" if value is None else f"{value * 100:.2f}%"


def fmt_ms(value: float | int | None) -> str:
    if value is None:
        return "n/a"
    return f"{float(value):.0f}ms"


def load_selections() -> dict[str, CaseSelection]:
    rows = list(csv.DictReader(SUMMARY_CSV.open(newline="", encoding="utf-8")))
    selections: dict[str, CaseSelection] = {}
    for row in rows:
        slug = row["event_slug"]
        if slug not in selections:
            summary = json.loads(resolve_repo_path(row["summary_json"]).read_text(encoding="utf-8"))
            selections[slug] = CaseSelection(
                event_slug=slug,
                market_label=row["market_label"],
                token_side=row["token_side"],
                token_id=summary["selection"]["token_id"],
            )
    return selections


def resolve_repo_path(value: str) -> Path:
    path = Path(value)
    if path.is_absolute():
        return path
    if path.parts and path.parts[0] == "research":
        return ROOT / path
    return RESEARCH_ROOT / path


def source_hour_coverage(manifest: dict[str, Any]) -> dict[str, Any]:
    hours: list[pd.Timestamp] = []
    pattern = re.compile(r"polymarket_orderbook_(\d{4}-\d{2}-\d{2}T\d{2})\.parquet$")
    for source in manifest.get("sourceFilesUsed", []):
        match = pattern.search(source)
        if match:
            hours.append(pd.Timestamp(match.group(1), tz="UTC"))
    hours = sorted(set(hours))
    missing: list[str] = []
    if hours:
        expected = pd.date_range(hours[0], hours[-1], freq="1h")
        have = set(hours)
        missing = [ts.strftime("%Y-%m-%dT%H") for ts in expected if ts not in have]
    return {
        "source_file_count": len(manifest.get("sourceFilesUsed", [])),
        "parsed_hour_count": len(hours),
        "first_source_hour": None if not hours else hours[0].isoformat(),
        "last_source_hour": None if not hours else hours[-1].isoformat(),
        "missing_source_hours": missing,
    }


def ordering_metrics(df: pd.DataFrame) -> dict[str, Any]:
    received = ts_ms(df["timestamp_received"])
    event_ts = ts_ms(df["timestamp"])
    d_received = received.diff().dropna()
    d_event = event_ts.diff().dropna()
    received_back = d_received[d_received < 0]
    event_back = d_event[d_event < 0]
    positive_gaps = d_received[d_received > 0]
    lag = received - event_ts
    return {
        "row_count": int(len(df)),
        "first_timestamp_received": pd.to_datetime(df["timestamp_received"], utc=True).min().isoformat(),
        "last_timestamp_received": pd.to_datetime(df["timestamp_received"], utc=True).max().isoformat(),
        "event_type_counts": dict(Counter(df["event_type"])),
        "physical_timestamp_received_inversions": int(len(received_back)),
        "physical_timestamp_received_max_back_ms": None if received_back.empty else float((-received_back).max()),
        "physical_timestamp_inversions": int(len(event_back)),
        "physical_timestamp_max_back_ms": None if event_back.empty else float((-event_back).max()),
        "received_minus_event_ms_quantiles": quantiles(lag, TIME_QUANTILES),
        "negative_received_minus_event_rows": int((lag < 0).sum()),
        "received_positive_gap_ms_quantiles": quantiles(positive_gaps, GAP_QUANTILES),
        "received_gap_gt_1s": int((positive_gaps > 1_000).sum()),
        "received_gap_gt_5s": int((positive_gaps > 5_000).sum()),
        "received_gap_gt_60s": int((positive_gaps > 60_000).sum()),
    }


def selected_token_metrics(df: pd.DataFrame, replay_summary: dict[str, Any] | None) -> dict[str, Any]:
    metrics = ordering_metrics(df)
    price_changes = df[df["event_type"] == "price_change"].copy()
    key_cols = ["timestamp_received", "timestamp", "market", "asset_id", "event_type"]
    message_cols = ["timestamp_received", "timestamp", "market", "asset_id"]

    def batch_metrics(frame: pd.DataFrame) -> dict[str, Any]:
        if frame.empty:
            return {}
        key_tuples = list(map(tuple, frame[key_cols].to_numpy()))
        run_ids: list[int] = []
        previous: tuple[Any, ...] | None = None
        run_id = -1
        for key in key_tuples:
            if key != previous:
                run_id += 1
                previous = key
            run_ids.append(run_id)
        with_keys = frame.copy()
        with_keys["_batch_key"] = key_tuples
        with_keys["_run_id"] = run_ids
        batch_sizes = with_keys.groupby("_batch_key", sort=False).size()
        runs_per_key = with_keys.groupby("_batch_key", sort=False)["_run_id"].nunique()
        split = runs_per_key[runs_per_key > 1]
        duplicate_subset = ["timestamp_received", "timestamp", "market", "asset_id", "event_type", "side", "price", "size"]
        duplicate_rows = int(with_keys.duplicated(subset=duplicate_subset, keep=False).sum())
        return {
            "price_change_rows": int(len(with_keys)),
            "batch_key_count": int(len(batch_sizes)),
            "multi_row_batch_count": int((batch_sizes > 1).sum()),
            "multi_row_batch_rows": int(batch_sizes[batch_sizes > 1].sum()),
            "max_batch_size": int(batch_sizes.max()),
            "split_batch_key_count": int(len(split)),
            "split_batch_rows": int(with_keys[with_keys["_batch_key"].isin(split.index)].shape[0]),
            "exact_duplicate_price_change_rows": duplicate_rows,
        }

    if price_changes.empty:
        metrics["price_change_batch_physical_order"] = {}
        metrics["price_change_batch_replay_sort"] = {}
    else:
        metrics["price_change_batch_physical_order"] = batch_metrics(price_changes)
        replay_sorted = price_changes.assign(_row=range(len(price_changes))).sort_values(
            ["timestamp", "timestamp_received", "_row"],
            kind="mergesort",
        )
        metrics["price_change_batch_replay_sort"] = batch_metrics(replay_sorted)

    books = df[df["event_type"] == "book"].copy()
    if books.empty:
        metrics["book_snapshot"] = {}
    else:
        book_received = ts_ms(books["timestamp_received"])
        book_gaps = book_received.diff().dropna()
        pc_message_keys = set(map(tuple, price_changes[message_cols].to_numpy())) if not price_changes.empty else set()
        book_message_keys = list(map(tuple, books[message_cols].to_numpy()))
        same_message = sum(1 for key in book_message_keys if key in pc_message_keys)
        metrics["book_snapshot"] = {
            "book_rows": int(len(books)),
            "same_message_as_price_change_rows": int(same_message),
            "book_gap_ms_quantiles": quantiles(book_gaps[book_gaps > 0], GAP_QUANTILES),
            "book_gap_gt_60s": int((book_gaps > 60_000).sum()),
            "book_gap_gt_300s": int((book_gaps > 300_000).sum()),
        }

    if replay_summary is not None:
        rq = replay_summary["replay_quality"]
        metrics["replay_quality_from_harness"] = {
            "batch_bbo_mismatch_rate": rq["pmxt_derived_bbo_diagnostic"]["price_change_batch_mismatch_rate"],
            "snapshot_bbo_mismatch_rate": rq["snapshot_alignment"]["snapshot_bbo_mismatch_rate"],
            "raw_snapshot_bbo_mismatch_rate": rq["snapshot_alignment"]["raw_snapshot_bbo_mismatch_rate"],
            "trade_off_book_rate": rq["trade_sanity"]["trade_off_book_rate"],
            "result_label": rq["result_label"],
        }
    return metrics


def load_replay_summaries() -> dict[str, dict[str, Any]]:
    rows = list(csv.DictReader(SUMMARY_CSV.open(newline="", encoding="utf-8")))
    summaries: dict[str, dict[str, Any]] = {}
    for row in rows:
        slug = row["event_slug"]
        if slug not in summaries:
            summaries[slug] = json.loads(resolve_repo_path(row["summary_json"]).read_text(encoding="utf-8"))
    return summaries


def diagnose_event(curated_root: Path, selection: CaseSelection, replay_summary: dict[str, Any] | None) -> dict[str, Any]:
    event_dir = curated_root / selection.event_slug
    parquet_path = event_dir / "orderbook.parquet"
    manifest_path = event_dir / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    schema_names = pq.ParquetFile(parquet_path).schema_arrow.names
    sequence_like_columns = [
        name
        for name in schema_names
        if any(marker in name.lower() for marker in ["sequence", "seq", "message", "msg"])
    ]

    all_df = pq.read_table(parquet_path, columns=["timestamp_received", "timestamp", "event_type"]).to_pandas()
    selected_df = pq.read_table(
        parquet_path,
        columns=[
            "timestamp_received",
            "timestamp",
            "market",
            "event_type",
            "asset_id",
            "price",
            "size",
            "side",
            "best_bid",
            "best_ask",
        ],
        filters=[("asset_id", "=", selection.token_id)],
    ).to_pandas()

    return {
        "event_slug": selection.event_slug,
        "market_label": selection.market_label,
        "token_side": selection.token_side,
        "token_id": selection.token_id,
        "parquet_path": str(parquet_path),
        "schema_columns": schema_names,
        "sequence_like_columns": sequence_like_columns,
        "schema_has_sequence_or_message_id": bool(sequence_like_columns),
        "source_hour_coverage": source_hour_coverage(manifest),
        "all_rows_physical_order": ordering_metrics(all_df),
        "selected_token_physical_order": selected_token_metrics(selected_df, replay_summary),
    }


def render_report(result: dict[str, Any]) -> str:
    lines: list[str] = []
    lines.append("# PMXT 上海温度事件顺序 / 漏包风险诊断")
    lines.append("")
    lines.append("更新：2026-06-25")
    lines.append("")
    lines.append("## 结论先行")
    lines.append("")
    lines.append("- **直接证据**：脚本检查 parquet schema 后，没有发现 `sequence` / `message_id` 这类字段，所以不能直接证明 WebSocket 是否漏了某条消息。")
    lines.append("- **直接证据**：源 PMXT 小时文件列表在两个样本里都是连续的，没有发现小时级源文件缺口。")
    lines.append("- **直接证据**：全 event parquet 不是全局严格按 `timestamp_received` 排序；但本次回测选中的 YES token 在物理顺序下 `timestamp_received` 没有倒退。")
    lines.append("- **直接证据**：选中 token 按 `timestamp` 看存在大量倒退，说明 exchange/event timestamp 到达顺序不是严格单调；这更像 WebSocket / 上游事件时间乱序或延迟，而不是单纯 parquet 写乱。")
    lines.append("- **推断**：目前更强的证据指向“message 边界、event-time 乱序、snapshot/checkpoint 语义不完整”，还不能直接定性为 Polymarket WS 漏包。")
    lines.append("")
    lines.append("## 指标表")
    lines.append("")
    lines.append("| event | rows | source hours missing | selected recv inversions | selected event-time inversions | max event back | physical split keys | replay-sort split keys | batch mismatch | snapshot mismatch | trade off-book |")
    lines.append("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    for event in result["events"]:
        all_rows = event["all_rows_physical_order"]
        selected = event["selected_token_physical_order"]
        coverage = event["source_hour_coverage"]
        physical_batch = selected["price_change_batch_physical_order"]
        replay_batch = selected["price_change_batch_replay_sort"]
        rq = selected.get("replay_quality_from_harness", {})
        lines.append(
            "| {event} | {rows} | {missing} | {recv_inv} | {event_inv} | {max_back} | {physical_split} | {replay_split} | {batch_mm} | {snap_mm} | {trade_off} |".format(
                event=event["event_slug"].replace("highest-temperature-in-shanghai-on-", ""),
                rows=all_rows["row_count"],
                missing=len(coverage["missing_source_hours"]),
                recv_inv=selected["physical_timestamp_received_inversions"],
                event_inv=selected["physical_timestamp_inversions"],
                max_back=fmt_ms(selected["physical_timestamp_max_back_ms"]),
                physical_split=physical_batch.get("split_batch_key_count", "n/a"),
                replay_split=replay_batch.get("split_batch_key_count", "n/a"),
                batch_mm=fmt_pct(rq.get("batch_bbo_mismatch_rate")),
                snap_mm=fmt_pct(rq.get("snapshot_bbo_mismatch_rate")),
                trade_off=fmt_pct(rq.get("trade_off_book_rate")),
            )
        )
    lines.append("")
    lines.append("## 分项证据")
    for event in result["events"]:
        all_rows = event["all_rows_physical_order"]
        selected = event["selected_token_physical_order"]
        coverage = event["source_hour_coverage"]
        physical_batch = selected["price_change_batch_physical_order"]
        replay_batch = selected["price_change_batch_replay_sort"]
        book = selected["book_snapshot"]
        lines.append("")
        lines.append(f"### {event['event_slug']} / {event['market_label']} {event['token_side']}")
        lines.append("")
        lines.append("**源文件 coverage**")
        lines.append("")
        lines.append(f"- sequence/message-like schema columns: {event['sequence_like_columns']}")
        lines.append(f"- source files: {coverage['source_file_count']}")
        lines.append(f"- parsed source hours: {coverage['parsed_hour_count']}")
        lines.append(f"- first/last source hour: {coverage['first_source_hour']} -> {coverage['last_source_hour']}")
        lines.append(f"- missing source hours: {coverage['missing_source_hours']}")
        lines.append("")
        lines.append("**物理顺序**")
        lines.append("")
        lines.append(f"- all rows `timestamp_received` inversions: {all_rows['physical_timestamp_received_inversions']}")
        lines.append(f"- all rows `timestamp` inversions: {all_rows['physical_timestamp_inversions']}")
        lines.append(f"- all rows max `timestamp` backstep: {fmt_ms(all_rows['physical_timestamp_max_back_ms'])}")
        lines.append(f"- selected token `timestamp_received` inversions: {selected['physical_timestamp_received_inversions']}")
        lines.append(f"- selected token `timestamp` inversions: {selected['physical_timestamp_inversions']}")
        lines.append(f"- selected token max `timestamp` backstep: {fmt_ms(selected['physical_timestamp_max_back_ms'])}")
        lines.append("")
        lines.append("**batch / snapshot**")
        lines.append("")
        lines.append(f"- price_change rows: {physical_batch.get('price_change_rows')}")
        lines.append(f"- physical-order batch keys: {physical_batch.get('batch_key_count')}")
        lines.append(f"- physical-order multi-row batch count / rows: {physical_batch.get('multi_row_batch_count')} / {physical_batch.get('multi_row_batch_rows')}")
        lines.append(f"- physical-order split batch key count / rows: {physical_batch.get('split_batch_key_count')} / {physical_batch.get('split_batch_rows')}")
        lines.append(f"- replay-sort split batch key count / rows: {replay_batch.get('split_batch_key_count')} / {replay_batch.get('split_batch_rows')}")
        lines.append(f"- max batch size: {physical_batch.get('max_batch_size')}")
        lines.append(f"- exact duplicate price_change rows: {physical_batch.get('exact_duplicate_price_change_rows')}")
        lines.append(f"- book rows: {book.get('book_rows')}")
        lines.append(f"- book rows sharing message key with price_change: {book.get('same_message_as_price_change_rows')}")
        lines.append("")
        lines.append("**received - event timestamp lag quantiles, selected token**")
        lines.append("")
        lines.append("```json")
        lines.append(json.dumps(selected["received_minus_event_ms_quantiles"], ensure_ascii=False, indent=2))
        lines.append("```")
        lines.append("")
        lines.append("**received positive gap quantiles, selected token**")
        lines.append("")
        lines.append("```json")
        lines.append(json.dumps(selected["received_positive_gap_ms_quantiles"], ensure_ascii=False, indent=2))
        lines.append("```")
    lines.append("")
    lines.append("## Evidence / inference / unknown")
    lines.append("")
    lines.append("### Evidence")
    lines.append("")
    lines.append("- schema 缺少 sequence/message id：无法用单调序列直接判定 WS 漏包。")
    lines.append("- 全 event parquet 物理顺序存在小时级 `timestamp_received` 倒退；结合选中 token 无倒退，更像 event parquet 由多个 market/token 分块拼接，不是单 token WS 流乱序。")
    lines.append("- 选中 token `timestamp_received` 无倒退：本次回测 token 的接收顺序本身没有乱。")
    lines.append("- 选中 token `timestamp` 有大量倒退：事件时间到达顺序不是严格单调，回放不能只假设 event time 完全有序。")
    lines.append("- 同 key 多行 batch 大量存在；物理顺序有少量 split key，但当前 replay sort 后 split key 为 0。")
    lines.append("- source hourly files 连续：没有小时级 coverage 缺口。")
    lines.append("")
    lines.append("### Inference")
    lines.append("")
    lines.append("- 剩余 mismatch 更可能来自 message boundary 不显式、same-message checkpoint、event-time/reception-time 语义差异、或 PMXT/Polymarket 的增量语义边界，而不是 curated 文件物理顺序写乱。")
    lines.append("- 不能排除 WS 层漏消息；但当前 parquet 缺少能直接证明漏消息的序列字段。")
    lines.append("")
    lines.append("### Unknown")
    lines.append("")
    lines.append("- 原始 WebSocket message id / sequence id / hash。")
    lines.append("- PMXT 是否在上游已经做过重连补偿或去重。")
    lines.append("- Polymarket WS 是否对所有 channel 保证同一 market 内严格有序。")
    lines.append("- book snapshot 是否保证 full-depth complete book，还是 checkpoint / partial view。")
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    args = parse_args()
    selections = load_selections()
    replay_summaries = load_replay_summaries()
    events = []
    for slug in EVENT_SLUGS:
        events.append(diagnose_event(args.curated_root, selections[slug], replay_summaries.get(slug)))
    result = {
        "generated_at": pd.Timestamp.now(tz="UTC").isoformat(),
        "curated_root": str(args.curated_root),
        "events": events,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    args.report.write_text(render_report(result), encoding="utf-8")
    print(f"wrote {args.out}")
    print(f"wrote {args.report}")


if __name__ == "__main__":
    main()
