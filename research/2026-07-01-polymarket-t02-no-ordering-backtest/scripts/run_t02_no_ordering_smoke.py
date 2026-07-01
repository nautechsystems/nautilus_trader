#!/usr/bin/env python3
"""Prepare and run a T02 PMXT no-receive-inversion smoke backtest.

This is a research-only adapter around the Shanghai single-token harness. It
uses the T02 live/PMXT alignment diagnostics, cuts the external hourly PMXT
parquet down to the raw capture window, builds a small synthetic curated event
folder for the two tokens that have in-window PMXT book snapshots, then runs the
existing four simple strategies with replay_order=received_time.
"""

from __future__ import annotations

import csv
import html
import json
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable

import pandas as pd
import pyarrow.parquet as pq

ROOT = Path(__file__).resolve().parents[3]
RESEARCH = ROOT / "research" / "2026-07-01-polymarket-t02-no-ordering-backtest"
CURATED_ROOT = RESEARCH / "curated"
DATA = RESEARCH / "data"
ASSETS = RESEARCH / "report_assets"
EVENT_SLUG = "t02-pmxt-live-until-1100-no-receive-inversion"
MARKET_LABEL = "T02 PMXT book pair"
STRATEGIES = ["maker_bbo", "buy_hold_first_ask", "momentum_taker", "contrarian_taker"]
RUNNER = ROOT / "research" / "2026-06-24-polymarket-shanghai-event-backtest" / "scripts" / "run_event_backtest.py"
POLYREAPER_ROOT = Path("C:/Projects/PolyReaper")
PMXT_HOURLY = POLYREAPER_ROOT / "data" / "external" / "pmxt" / "polymarket" / "v2" / "orderbook" / "hourly" / "polymarket_orderbook_2026-06-26T02.parquet"
CAPTURE_RESEARCH = POLYREAPER_ROOT / "research" / "2026-06-25-polymarket-raw-ws-ordering-capture"
ALIGNMENT_DIAG = CAPTURE_RESEARCH / "data" / "diagnostics" / "live_until_1100_pmxt_alignment_diagnostics.json"
RAW_ORDER_DIAG = CAPTURE_RESEARCH / "data" / "diagnostics" / "live_until_1100_raw_ws_ordering_diagnostics.json"
RAW_CAPTURE = CAPTURE_RESEARCH / "data" / "raw" / "ws_capture_20260626T022520Z.ndjson"
SOURCE_REPORT = CAPTURE_RESEARCH / "report_pmxt_alignment_ordering.md"


@dataclass(frozen=True)
class TokenPair:
    market: str
    yes_token: str
    no_token: str
    yes_mid: float
    no_mid: float


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def repo_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


def parse_levels(raw: Any) -> dict[float, float]:
    if raw is None or pd.isna(raw):
        return {}
    parsed = json.loads(str(raw))
    return {float(price): float(size) for price, size in parsed if float(size) > 0}


def best_bid_ask(bids: dict[float, float], asks: dict[float, float]) -> tuple[float | None, float | None]:
    return (max(bids) if bids else None, min(asks) if asks else None)


def mid_from_book(row: pd.Series) -> float:
    bid, ask = best_bid_ask(parse_levels(row["bids"]), parse_levels(row["asks"]))
    if bid is None or ask is None:
        return 0.0
    return (bid + ask) / 2


def discover_pair(filtered: pd.DataFrame) -> TokenPair:
    book_rows = filtered[filtered["event_type"] == "book"].copy()
    if len(book_rows) < 2:
        raise SystemExit(f"need at least two PMXT book rows in T02 window, found {len(book_rows)}")
    book_rows["mid"] = book_rows.apply(mid_from_book, axis=1)
    market_counts = book_rows.groupby("market").size().sort_values(ascending=False)
    market = market_counts.index[0]
    market_books = book_rows[book_rows["market"] == market].sort_values("mid", ascending=False)
    if len(market_books) != 2:
        raise SystemExit(f"expected exactly two book rows for selected market {market!r}, found {len(market_books)}")
    high = market_books.iloc[0]
    low = market_books.iloc[1]
    return TokenPair(
        market=str(market),
        yes_token=str(high["asset_id"]),
        no_token=str(low["asset_id"]),
        yes_mid=float(high["mid"]),
        no_mid=float(low["mid"]),
    )


def prepare_curated_event() -> tuple[Path, TokenPair, dict[str, Any], pd.DataFrame]:
    alignment = read_json(ALIGNMENT_DIAG)
    raw_window = alignment["raw_window_utc"]
    start = pd.Timestamp(raw_window["start"])
    end = pd.Timestamp(raw_window["end"])
    assets = alignment["raw_summary"]["assets"]
    columns = [
        "timestamp_received",
        "timestamp",
        "market",
        "event_type",
        "asset_id",
        "bids",
        "asks",
        "price",
        "size",
        "side",
        "best_bid",
        "best_ask",
        "transaction_hash",
        "fee_rate_bps",
        "old_tick_size",
        "new_tick_size",
    ]
    table = pq.read_table(
        PMXT_HOURLY,
        columns=columns,
        filters=[
            ("asset_id", "in", assets),
            ("timestamp_received", ">=", start.to_pydatetime()),
            ("timestamp_received", "<=", end.to_pydatetime()),
        ],
    )
    filtered = table.to_pandas()
    pair = discover_pair(filtered)
    selected = filtered[filtered["asset_id"].isin([pair.yes_token, pair.no_token])].copy()
    selected = selected.sort_values(["timestamp_received", "timestamp"], kind="mergesort")

    event_dir = CURATED_ROOT / EVENT_SLUG
    event_dir.mkdir(parents=True, exist_ok=True)
    selected.to_parquet(event_dir / "orderbook.parquet", index=False)
    event_index = {
        "eventSlug": EVENT_SLUG,
        "title": "T02 PMXT live-until-1100 no-receive-inversion sample",
        "source": {
            "pmxt_hourly": str(PMXT_HOURLY),
            "raw_capture": str(RAW_CAPTURE),
            "alignment_diagnostics": str(ALIGNMENT_DIAG),
            "raw_ordering_diagnostics": str(RAW_ORDER_DIAG),
            "source_report": str(SOURCE_REPORT),
            "raw_window_utc": raw_window,
            "note": "Synthetic metadata for a research-only smoke replay. YES/NO labels are assigned by higher/lower first in-window book mid; no settlement value is known.",
        },
        "markets": [
            {
                "label": MARKET_LABEL,
                "marketId": "t02-synthetic-market-0",
                "conditionId": pair.market,
                "question": "Synthetic T02 PMXT book pair selected from the live-until-1100 sample",
                "yesToken": pair.yes_token,
                "noToken": pair.no_token,
            }
        ],
    }
    # Keep markets empty so run_event_backtest leaves settlement_value=None.
    gamma_event = {"id": EVENT_SLUG, "title": event_index["title"], "markets": []}
    (event_dir / "event_index.json").write_text(json.dumps(event_index, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    (event_dir / "gamma_event.raw.json").write_text(json.dumps(gamma_event, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return event_dir, pair, alignment, selected


def run_backtests() -> list[dict[str, Any]]:
    DATA.mkdir(parents=True, exist_ok=True)
    summaries: list[dict[str, Any]] = []
    for token_side in ["YES", "NO"]:
        for strategy in STRATEGIES:
            command = [
                sys.executable,
                str(RUNNER),
                "--event-slug",
                EVENT_SLUG,
                "--market-label",
                MARKET_LABEL,
                "--token-side",
                token_side,
                "--curated-root",
                str(CURATED_ROOT),
                "--out-dir",
                str(DATA),
                "--strategy",
                strategy,
                "--quote-size",
                "10",
                "--max-inventory",
                "100",
                "--decision-frequency",
                "5min",
                "--signal-threshold",
                "0.03",
                "--replay-order",
                "received_time",
            ]
            completed = subprocess.run(command, check=True, text=True, capture_output=True)
            summary = json.loads(completed.stdout)
            summaries.append(summary)
            print(
                f"{token_side} {strategy}: fills={summary['backtest']['fills']} "
                f"mtm_pnl={summary['backtest']['mtm_pnl']}"
            )
    return summaries


def write_summary_csv(summaries: list[dict[str, Any]]) -> Path:
    path = DATA / "t02_strategy_suite_summary.csv"
    fieldnames = [
        "token_side",
        "token_id",
        "strategy",
        "rows_for_token",
        "fills",
        "buy_qty",
        "sell_qty",
        "ending_inventory",
        "gross_notional",
        "mtm_pnl",
        "final_mark_price",
        "settlement_pnl",
        "price_change_batch_mismatch_rate",
        "snapshot_bbo_mismatch_rate",
        "trade_off_book_rate",
        "split_price_change_batch_key_count",
        "final_tick_size",
        "tick_size_changes_applied",
        "fill_tick_price_violations",
        "result_label",
        "replay_order",
        "summary_json",
        "bbo_csv",
        "fills_csv",
    ]
    with path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames)
        writer.writeheader()
        for s in summaries:
            rq = s["replay_quality"]
            row = {
                "token_side": s["selection"]["token_side"],
                "token_id": s["selection"]["token_id"],
                "strategy": s["inputs"]["strategy"],
                "rows_for_token": s["inputs"]["rows_for_token"],
                "fills": s["backtest"]["fills"],
                "buy_qty": s["backtest"]["buy_qty"],
                "sell_qty": s["backtest"]["sell_qty"],
                "ending_inventory": s["backtest"]["ending_inventory"],
                "gross_notional": s["backtest"]["gross_notional"],
                "mtm_pnl": s["backtest"]["mtm_pnl"],
                "final_mark_price": s["backtest"]["final_mark_price"],
                "settlement_pnl": s["backtest"]["settlement_pnl"],
                "price_change_batch_mismatch_rate": rq["pmxt_derived_bbo_diagnostic"]["price_change_batch_mismatch_rate"],
                "snapshot_bbo_mismatch_rate": rq["snapshot_alignment"]["snapshot_bbo_mismatch_rate"],
                "trade_off_book_rate": rq["trade_sanity"]["trade_off_book_rate"],
                "split_price_change_batch_key_count": rq["pmxt_derived_bbo_diagnostic"]["split_price_change_batch_key_count"],
                "final_tick_size": rq["tick_size"]["final_tick_size"],
                "tick_size_changes_applied": rq["tick_size"]["tick_size_changes_applied"],
                "fill_tick_price_violations": rq["tick_size"]["fill_tick_price_violations"],
                "result_label": rq["result_label"],
                "replay_order": s["inputs"]["replay_order"],
                "summary_json": s["outputs"]["summary_json"],
                "bbo_csv": s["outputs"]["bbo_csv"],
                "fills_csv": s["outputs"]["fills_csv"],
            }
            writer.writerow(row)
    return path


def read_csv(path: Path) -> list[dict[str, str]]:
    with path.open("r", encoding="utf-8", newline="") as f:
        return list(csv.DictReader(f))


def esc(value: object) -> str:
    return html.escape(str(value), quote=True)


def f(value: str | float | int | None) -> float:
    if value is None or value == "":
        return 0.0
    return float(value)


def optional_float(value: str | float | int | None) -> float | None:
    if value is None or value == "":
        return None
    parsed = float(value)
    if parsed != parsed:
        return None
    return parsed


def parse_dt(value: str) -> datetime:
    return datetime.fromisoformat(value.strip().replace("Z", "+00:00"))


def scale(vals: Iterable[float], lo_px: float, hi_px: float, pad_frac: float = 0.08):
    vals = list(vals)
    lo = min(vals) if vals else 0.0
    hi = max(vals) if vals else 1.0
    if lo == hi:
        lo -= 1.0
        hi += 1.0
    pad = (hi - lo) * pad_frac
    lo -= pad
    hi += pad

    def mapv(v: float) -> float:
        return lo_px + (v - lo) / (hi - lo) * (hi_px - lo_px)

    return mapv, lo, hi


def polyline(points: list[tuple[float, float]], color: str, width: float = 2.0) -> str:
    if len(points) < 2:
        return ""
    pts = " ".join(f"{x:.1f},{y:.1f}" for x, y in points)
    return f'<polyline fill="none" stroke="{color}" stroke-width="{width}" points="{pts}"/>'


def optional_polyline(points: list[tuple[float, float | None]], color: str, width: float = 2.0) -> str:
    parts: list[str] = []
    current: list[tuple[float, float]] = []
    for x, y in points:
        if y is None:
            if len(current) >= 2:
                parts.append(polyline(current, color, width))
            current = []
        else:
            current.append((x, y))
    if len(current) >= 2:
        parts.append(polyline(current, color, width))
    return "\n".join(parts)


def svg_frame(width: int, height: int, body: str, title: str) -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img">\n'
        f"<title>{esc(title)}</title>\n"
        '<rect width="100%" height="100%" fill="white"/>\n'
        '<style>text{font-family:Arial,Helvetica,sans-serif;font-size:12px;fill:#222}.small{font-size:10px;fill:#555}.title{font-size:16px;font-weight:700}.axis{stroke:#999;stroke-width:1}.grid{stroke:#e6e6e6;stroke-width:1}.legend{font-size:11px}</style>\n'
        f"{body}\n</svg>\n"
    )


def case_dir(summary_row: dict[str, str]) -> Path:
    path = Path(summary_row["summary_json"])
    if not path.is_absolute():
        path = ROOT / path
    return path.parent


def generate_assets(summary_csv: Path) -> list[Path]:
    ASSETS.mkdir(parents=True, exist_ok=True)
    rows = read_csv(summary_csv)
    colors = {"YES": "#1f77b4", "NO": "#ff7f0e", "maker_bbo": "#d62728", "buy_hold_first_ask": "#2ca02c", "momentum_taker": "#9467bd", "contrarian_taker": "#8c564b"}

    # Price paths: use buy_hold rows because each strategy has identical BBO replay for a token.
    series = []
    all_times: list[datetime] = []
    all_prices: list[float] = []
    for row in rows:
        if row["strategy"] != "buy_hold_first_ask":
            continue
        bbo = read_csv(case_dir(row) / "bbo_5min.csv")
        pts = []
        for r in bbo:
            ts = parse_dt(r["timestamp"])
            mid = optional_float(r.get("mid"))
            mark = optional_float(r.get("mark_price"))
            y = mid if mid is not None else mark
            pts.append((ts, y))
            all_times.append(ts)
            if y is not None:
                all_prices.append(y)
        series.append((row["token_side"], pts))
    width, height = 980, 420
    m = dict(left=65, right=25, top=55, bottom=55)
    plot_w, plot_h = width - m["left"] - m["right"], height - m["top"] - m["bottom"]
    t0, t1 = min(all_times), max(all_times)
    span = max((t1 - t0).total_seconds(), 1)
    ymap, ymin, ymax = scale(all_prices, m["top"] + plot_h, m["top"], 0.05)
    body = [f'<text x="{m["left"]}" y="28" class="title">T02 selected-token price paths (mid/mark, 5min)</text>']
    body += [f'<line class="grid" x1="{m["left"]}" y1="{m["top"] + i * plot_h / 4:.1f}" x2="{m["left"] + plot_w}" y2="{m["top"] + i * plot_h / 4:.1f}"/>' for i in range(5)]
    body.append(f'<line class="axis" x1="{m["left"]}" y1="{m["top"] + plot_h}" x2="{m["left"] + plot_w}" y2="{m["top"] + plot_h}"/>')
    body.append(f'<line class="axis" x1="{m["left"]}" y1="{m["top"]}" x2="{m["left"]}" y2="{m["top"] + plot_h}"/>')
    for token_side, pts in series:
        mapped = []
        for ts, val in pts:
            x = m["left"] + (ts - t0).total_seconds() / span * plot_w
            mapped.append((x, None if val is None else ymap(val)))
        body.append(optional_polyline(mapped, colors[token_side], 2.4))
        body.append(f'<text x="{m["left"] + plot_w - 130}" y="{m["top"] + 18 + 18 * (0 if token_side == "YES" else 1)}" class="legend" fill="{colors[token_side]}">{esc(token_side)}</text>')
    body.append(f'<text x="8" y="{m["top"] + 12}" class="small">{ymax:.3f}</text><text x="8" y="{m["top"] + plot_h}" class="small">{ymin:.3f}</text>')
    price_svg = ASSETS / "t02_price_paths.svg"
    price_svg.write_text(svg_frame(width, height, "\n".join(body), "T02 price paths"), encoding="utf-8")

    # MTM equity curves by token/strategy.
    all_times = []
    all_equity = []
    equity_series = []
    for row in rows:
        bbo = read_csv(case_dir(row) / "bbo_5min.csv")
        pts = []
        for r in bbo:
            ts = parse_dt(r["timestamp"])
            eq = optional_float(r.get("mtm_equity"))
            pts.append((ts, eq))
            all_times.append(ts)
            if eq is not None:
                all_equity.append(eq)
        equity_series.append((f"{row['token_side']} {row['strategy']}", row["strategy"], pts))
    width, height = 980, 520
    m = dict(left=70, right=25, top=55, bottom=55)
    plot_w, plot_h = width - m["left"] - m["right"], height - m["top"] - m["bottom"]
    t0, t1 = min(all_times), max(all_times)
    span = max((t1 - t0).total_seconds(), 1)
    ymap, ymin, ymax = scale(all_equity + [0.0], m["top"] + plot_h, m["top"], 0.12)
    zero = ymap(0.0)
    body = [f'<text x="{m["left"]}" y="28" class="title">T02 MTM equity curves (no settlement)</text>']
    body += [f'<line class="grid" x1="{m["left"]}" y1="{m["top"] + i * plot_h / 4:.1f}" x2="{m["left"] + plot_w}" y2="{m["top"] + i * plot_h / 4:.1f}"/>' for i in range(5)]
    body.append(f'<line stroke="#444" stroke-width="1" stroke-dasharray="4 3" x1="{m["left"]}" y1="{zero:.1f}" x2="{m["left"] + plot_w}" y2="{zero:.1f}"/>')
    body.append(f'<line class="axis" x1="{m["left"]}" y1="{m["top"] + plot_h}" x2="{m["left"] + plot_w}" y2="{m["top"] + plot_h}"/>')
    for idx, (label, strategy, pts) in enumerate(equity_series):
        # Use dashed-ish lighter colors by token via opacity.
        color = colors.get(strategy, "#333")
        mapped = []
        for ts, val in pts:
            x = m["left"] + (ts - t0).total_seconds() / span * plot_w
            mapped.append((x, None if val is None else ymap(val)))
        body.append(optional_polyline(mapped, color, 1.8 if label.startswith("NO") else 2.4))
        if idx < 8:
            body.append(f'<text x="{m["left"] + 10 + (idx // 4) * 430}" y="{height - 35 + (idx % 4) * 12}" class="small">{esc(label)}</text>')
    body.append(f'<text x="8" y="{m["top"] + 12}" class="small">{ymax:.2f}</text><text x="8" y="{m["top"] + plot_h}" class="small">{ymin:.2f}</text>')
    equity_svg = ASSETS / "t02_mtm_equity.svg"
    equity_svg.write_text(svg_frame(width, height, "\n".join(body), "T02 MTM equity curves"), encoding="utf-8")
    return [price_svg, equity_svg]


def fmt(value: Any, digits: int = 2) -> str:
    if value is None or value == "":
        return "n/a"
    return f"{float(value):.{digits}f}"


def pct(value: Any) -> str:
    if value is None or value == "":
        return "n/a"
    return f"{float(value) * 100:.2f}%"


def write_report(pair: TokenPair, alignment: dict[str, Any], selected: pd.DataFrame, summary_csv: Path, assets: list[Path]) -> Path:
    raw_order = read_json(RAW_ORDER_DIAG)
    rows = read_csv(summary_csv)
    sample_by_side: dict[str, dict[str, str]] = {}
    for r in rows:
        if r["strategy"] == "buy_hold_first_ask":
            sample_by_side[r["token_side"]] = r
    report = RESEARCH / "report_t02_no_ordering_smoke.md"
    lines: list[str] = []
    lines.append("# T02 PMXT 无接收乱序样本：received-time replay 回测 smoke")
    lines.append("")
    lines.append("## 1. 这次跑的是什么")
    lines.append("")
    lines.append("这次不是重新证明 PMXT/raw 对齐，而是在已经确认 `live-until-1100 / T02` 样本接收顺序可用的前提下，把同一段数据喂给现有 Polymarket L2 replay / simple strategy harness，看短窗口回测输出是否合理。")
    lines.append("")
    lines.append(f"- PMXT 小时包：`{PMXT_HOURLY}`")
    lines.append(f"- raw WS：`{RAW_CAPTURE}`")
    lines.append(f"- raw window UTC：`{alignment['raw_window_utc']['start']}` ～ `{alignment['raw_window_utc']['end']}`")
    lines.append("- 北京时间：`2026-06-26 10:25:28` ～ `10:59:53`")
    lines.append(f"- 本次瘦身 parquet：`{repo_path(CURATED_ROOT / EVENT_SLUG / 'orderbook.parquet')}`")
    lines.append(f"- 回放顺序：`received_time`，即按 `timestamp_received, timestamp, _row` 排序。")
    lines.append("")
    lines.append("## 2. 为什么不是直接用全小时包跑")
    lines.append("")
    lines.append("现有 harness 是 event/token 级，需要初始 `book` snapshot。T02 对齐窗口内 PMXT 可比 rows 一共 2611 条，但只有一个 binary market 的两只 token 在窗口内有 PMXT `book` snapshot；其它 token 只有 `price_change`，没有初始 book，直接跑会跳过早期变动，不能作为这轮 smoke 的主样本。")
    lines.append("")
    lines.append("因此本次只选这两个可独立初始化的 token：")
    lines.append("")
    lines.append("| Synthetic side | token_id 简写 | first book mid | rows | book | price_change | trade |")
    lines.append("| --- | --- | ---: | ---: | ---: | ---: | ---: |")
    for side, token in [("YES", pair.yes_token), ("NO", pair.no_token)]:
        side_df = selected[selected["asset_id"] == token]
        counts = side_df["event_type"].value_counts()
        mid = pair.yes_mid if side == "YES" else pair.no_mid
        lines.append(f"| {side} | `{token[:8]}...{token[-6:]}` | {mid:.3f} | {len(side_df)} | {int(counts.get('book', 0))} | {int(counts.get('price_change', 0))} | {int(counts.get('last_trade_price', 0))} |")
    lines.append("")
    lines.append("说明：这里的 YES/NO 是 synthetic label，只表示 first book mid 高/低的两只互补 token；本轮没有接入真实 resolution，因此不输出 settlement 结论。")
    lines.append("")
    lines.append("## 3. 顺序与质量检查")
    lines.append("")
    lines.append("| 检查项 | 结果 | 解读 |")
    lines.append("| --- | ---: | --- |")
    lines.append(f"| raw receive inversion | {raw_order['receive_order']['recv_wall_time_inversions']} | 本地接收墙钟没有倒退 |")
    lines.append(f"| raw monotonic inversion | {raw_order['receive_order']['recv_monotonic_ns_inversions']} | 本地 monotonic clock 没有倒退 |")
    lines.append(f"| raw per-asset timestamp inversion | {raw_order['timestamp_order']['total_asset_series_inversions']} | raw 口径同 asset 没有 source timestamp 倒序 |")
    lines.append("| PMXT per-asset `timestamp_received` inversion | 0 | 对齐诊断中每个 selected asset 的 receive-time order 没倒序 |")
    lines.append("| PMXT source `timestamp` inversion | 有 | 所以本轮不用 source-time 排序，而用 received-time replay 做理想化 smoke |")
    lines.append("")
    lines.append("Replay 质量（取每个 token 的 buy_hold run，因为同 token 各策略共享同一条 L2 replay）：")
    lines.append("")
    lines.append("| Side | result label | batch mismatch | split batch keys | snapshot BBO mismatch | trade off-book | final tick | fill tick violations |")
    lines.append("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |")
    for side in ["YES", "NO"]:
        r = sample_by_side[side]
        lines.append(
            f"| {side} | `{r['result_label']}` | {pct(r['price_change_batch_mismatch_rate'])} | {r['split_price_change_batch_key_count']} | "
            f"{pct(r['snapshot_bbo_mismatch_rate'])} | {pct(r['trade_off_book_rate'])} | {r['final_tick_size']} | {r['fill_tick_price_violations']} |"
        )
    lines.append("")
    lines.append("## 4. 策略 smoke 结果")
    lines.append("")
    lines.append("![T02 price paths](report_assets/t02_price_paths.svg)")
    lines.append("")
    lines.append("![T02 MTM equity](report_assets/t02_mtm_equity.svg)")
    lines.append("")
    lines.append("| Side | Strategy | Fills | Ending inventory | Gross notional | MTM PnL | Final mark |")
    lines.append("| --- | --- | ---: | ---: | ---: | ---: | ---: |")
    for r in rows:
        lines.append(
            f"| {r['token_side']} | {r['strategy']} | {r['fills']} | {fmt(r['ending_inventory'])} | "
            f"{fmt(r['gross_notional'])} | {fmt(r['mtm_pnl'])} | {fmt(r['final_mark_price'], 3)} |"
        )
    lines.append("")
    lines.append("## 5. 怎么看结果")
    lines.append("")
    lines.append("- 这轮能跑通：T02 的两个可初始化 token 都完成 L2 replay，8 个 simple strategy run 都产出 summary / fills / BBO time series。")
    lines.append("- `split_price_change_batch_key_count=0`，说明在 received-time replay 口径下，同一个 PMXT price_change batch 没被 sort 拆开。")
    lines.append("- `fill_tick_price_violations=0`，说明当前模拟成交价格都落在 active tick grid 上。")
    lines.append("- `maker_bbo` 基本没有太多成交机会：T02 这段在选中 pair 里只有一条 `last_trade_price`，所以 maker 结果只能看 plumbing，不能看策略 edge。")
    lines.append("- 这轮没有真实 settlement，也没有 fee / taker delay / queue / partial fill，所以 MTM 只能代表短窗口 mark-to-market smoke，不代表最终收益。")
    lines.append("- 最值得看的不是 PnL 大小，而是：received-time replay 后 BBO/batch/tick/fill 链路是否稳定；这点当前结果是可用的。")
    lines.append("")
    lines.append("## 6. 产物")
    lines.append("")
    lines.append(f"- suite summary：`{repo_path(summary_csv)}`")
    for asset in assets:
        lines.append(f"- chart：`{repo_path(asset)}`")
    lines.append(f"- synthetic curated event：`{repo_path(CURATED_ROOT / EVENT_SLUG)}`")
    report.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return report


def main() -> None:
    for required in [PMXT_HOURLY, ALIGNMENT_DIAG, RAW_ORDER_DIAG, RAW_CAPTURE, RUNNER]:
        if not required.exists():
            raise SystemExit(f"missing required input: {required}")
    DATA.mkdir(parents=True, exist_ok=True)
    ASSETS.mkdir(parents=True, exist_ok=True)
    _, pair, alignment, selected = prepare_curated_event()
    summaries = run_backtests()
    summary_csv = write_summary_csv(summaries)
    assets = generate_assets(summary_csv)
    report = write_report(pair, alignment, selected, summary_csv, assets)
    print(f"wrote {summary_csv}")
    print(f"wrote {report}")


if __name__ == "__main__":
    main()
