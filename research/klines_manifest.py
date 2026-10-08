"""
Build a manifest of the Binance USD-M 1-minute kline archive and prune unusable data.

Layout expected: <data-dir>/<SYMBOL>/<SYMBOL>-1m-YYYY-MM.parquet

Pruning rules (only applied with --apply, otherwise a dry run):
- Non-crypto symbols: exchangeInfo `underlyingType` other than COIN (TradFi perpetuals on
  equities, commodities, FX, indices) or a COIN with the RWA subtype (e.g. PAXG tracks gold).
  Symbols missing from exchangeInfo were delisted crypto perpetuals and are kept.
- Dead months: every minute has zero trades (Binance keeps publishing frozen-price files
  after a contract is delisted).
- With --drop-delisted: every file of a symbol whose last traded month is before the
  archive's last month (delisted mid-sample). This introduces survivorship bias in backtests.
- With --drop-symbols: every file of the listed symbols.

Outputs written to <manifest-dir>:
- files.csv    one row per monthly file (rows, missing minutes, zero-trade minutes, dead flag)
- gaps.csv     runs of missing minutes across each kept symbol history (not before the
               first bar or after the last bar)
- symbols.csv  one row per symbol (category, kept months, first/last trade time)
- prune_plan.csv  files the current run deletes (with --apply) or would delete (dry run)
- pruned.csv   append-only log of every file deleted by --apply runs and why

Usage (from the repository root):
    python/.venv/Scripts/python research/klines_manifest.py            # dry run
    python/.venv/Scripts/python research/klines_manifest.py --apply    # delete

"""

from __future__ import annotations

import argparse
import calendar
import json
import re
from datetime import UTC
from datetime import datetime
from pathlib import Path

import polars as pl


MINUTE_MS = 60_000
FILE_RE = re.compile(r"^(?P<sym>[A-Z0-9]+)-1m-(?P<y>\d{4})-(?P<m>\d{2})\.parquet$")


def month_bounds_ms(year: int, month: int) -> tuple[int, int]:
    start = datetime(year, month, 1, tzinfo=UTC)
    days = calendar.monthrange(year, month)[1]
    start_ms = int(start.timestamp() * 1000)
    return start_ms, start_ms + days * 1440 * MINUTE_MS


def fmt_ms(ms: int | None) -> str | None:
    if ms is None:
        return None
    return datetime.fromtimestamp(ms / 1000, UTC).strftime("%Y-%m-%d %H:%M")


def classify(symbol: str, info: dict[str, dict]) -> str:
    s = info.get(symbol)
    if s is None:
        return "crypto_delisted"
    if s.get("underlyingType") != "COIN":
        return f"non_crypto_{s.get('underlyingType', 'UNKNOWN').lower()}"
    if "RWA" in (s.get("underlyingSubType") or []):
        return "non_crypto_rwa"
    return "crypto"


def scan_file(path: Path, symbol: str, year: int, month: int) -> tuple[dict, pl.Series]:
    df = pl.read_parquet(path, columns=["open_time", "count"]).sort("open_time")
    start_ms, end_ms = month_bounds_ms(year, month)
    expected = (end_ms - start_ms) // MINUTE_MS
    times = df["open_time"]
    traded = df.filter(pl.col("count") > 0)["open_time"]
    row = {
        "symbol": symbol,
        "month": f"{year:04d}-{month:02d}",
        "rows": df.height,
        "expected_rows": expected,
        "missing_minutes": expected - df.height,
        "duplicate_minutes": df.height - times.n_unique(),
        "zero_trade_minutes": int((df["count"] == 0).sum()),
        "first_bar": fmt_ms(int(times.min())) if df.height else None,
        "last_bar": fmt_ms(int(times.max())) if df.height else None,
        "first_trade": fmt_ms(int(traded.min())) if traded.len() else None,
        "last_trade": fmt_ms(int(traded.max())) if traded.len() else None,
        "dead": traded.len() == 0,
        "path": str(path),
    }
    return row, times


def find_gaps(symbol: str, times: pl.Series) -> list[dict]:
    # Gaps are measured across the symbol's whole kept history, so runs missing at a month
    # boundary are caught; minutes before the first bar or after the last bar are not gaps
    jumps = (
        pl.DataFrame({"t0": times.unique().sort()})
        .with_columns(pl.col("t0").diff().shift(-1).alias("step"))
        .filter(pl.col("step") > MINUTE_MS)
        .rows()
    )
    return [
        {
            "symbol": symbol,
            "gap_start": fmt_ms(t0 + MINUTE_MS),
            "gap_end": fmt_ms(t0 + step - MINUTE_MS),
            "missing_minutes": step // MINUTE_MS - 1,
        }
        for t0, step in jumps
    ]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[1])
    parser.add_argument("--data-dir", type=Path, default=Path("binance_data/klines_1m"))
    parser.add_argument("--manifest-dir", type=Path, default=Path("binance_data/manifest"))
    parser.add_argument(
        "--exchange-info",
        type=Path,
        default=Path("binance_data/manifest/exchange_info_um_2026-10-08.json"),
    )
    parser.add_argument("--apply", action="store_true", help="delete pruned files")
    parser.add_argument(
        "--drop-delisted",
        action="store_true",
        help="also prune symbols whose last traded month is before the archive's last month",
    )
    parser.add_argument(
        "--drop-symbols",
        nargs="+",
        default=[],
        metavar="SYMBOL",
        help="also prune every file of these symbols",
    )
    args = parser.parse_args()

    raw = json.loads(args.exchange_info.read_text(encoding="utf-8"))
    info = {s["symbol"]: s for s in raw["symbols"]}

    file_rows: list[dict] = []
    gap_rows: list[dict] = []
    for sym_dir in sorted(p for p in args.data_dir.iterdir() if p.is_dir()):
        category = classify(sym_dir.name, info)
        kept_times = []
        for path in sorted(sym_dir.glob("*.parquet")):
            m = FILE_RE.match(path.name)
            if m is None or m["sym"] != sym_dir.name:
                raise ValueError(f"Unexpected file name: {path}")
            row, times = scan_file(path, sym_dir.name, int(m["y"]), int(m["m"]))
            row["category"] = category
            file_rows.append(row)
            if category.startswith("crypto") and not row["dead"]:
                kept_times.append(times)
        if kept_times:
            gap_rows.extend(find_gaps(sym_dir.name, pl.concat(kept_times)))

    files = pl.DataFrame(file_rows)
    live_last_month = (
        files.filter(~pl.col("dead"))
        .group_by("symbol")
        .agg(pl.col("month").max().alias("live_last"))
    )
    files = files.join(live_last_month, on="symbol", how="left").with_columns(
        pl.when(pl.col("category").str.starts_with("non_crypto"))
        .then(pl.col("category"))
        .when(pl.col("dead"))
        .then(pl.lit("dead_month"))
        .when(args.drop_delisted & (pl.col("live_last") < files["month"].max()))
        .then(pl.lit("delisted_before_end"))
        .when(pl.col("symbol").is_in(args.drop_symbols))
        .then(pl.lit("dropped_by_request"))
        .otherwise(None)
        .alias("prune_reason"),
    )
    files = files.drop("live_last")
    pruned = files.filter(pl.col("prune_reason").is_not_null())
    gaps = pl.DataFrame(
        gap_rows,
        schema={
            "symbol": pl.String,
            "gap_start": pl.String,
            "gap_end": pl.String,
            "missing_minutes": pl.Int64,
        },
    ).filter(
        pl.col("symbol").is_in(
            files.filter(pl.col("prune_reason").is_null())["symbol"].unique().implode(),
        ),
    )
    kept = files.filter(pl.col("prune_reason").is_null())

    symbols = (
        files.group_by("symbol")
        .agg(
            pl.col("category").first(),
            pl.col("month").min().alias("first_month"),
            pl.col("month").max().alias("last_month"),
            pl.len().alias("months_total"),
            pl.col("prune_reason").is_null().sum().alias("months_kept"),
            pl.col("dead").sum().alias("months_dead"),
            pl.col("first_trade").drop_nulls().min().alias("first_trade"),
            pl.col("last_trade").drop_nulls().max().alias("last_trade"),
        )
        .with_columns((pl.col("months_kept") > 0).alias("kept"))
        .join(
            gaps.group_by("symbol").agg(
                pl.len().alias("gap_runs"),
                pl.col("missing_minutes").sum().alias("gap_minutes"),
            ),
            on="symbol",
            how="left",
        )
        .with_columns(pl.col("gap_runs", "gap_minutes").fill_null(0))
        .sort("symbol")
    )

    args.manifest_dir.mkdir(parents=True, exist_ok=True)
    files.drop("path").write_csv(args.manifest_dir / "files.csv")
    gaps.write_csv(args.manifest_dir / "gaps.csv")
    symbols.write_csv(args.manifest_dir / "symbols.csv")
    plan = pruned.select("symbol", "month", "category", "prune_reason", "path")
    plan.write_csv(args.manifest_dir / "prune_plan.csv")

    print(f"files scanned: {files.height}, kept: {kept.height}, pruned: {pruned.height}")
    print(
        pruned.group_by("prune_reason")
        .agg(pl.len(), pl.col("symbol").n_unique())
        .sort("prune_reason")
    )
    print(f"symbols kept: {symbols['kept'].sum()} / {symbols.height}")
    print(f"gaps: {gaps.height} runs, {gaps['missing_minutes'].sum()} minutes")

    if not args.apply:
        print("dry run: nothing deleted (pass --apply to delete)")
        return

    for path in pruned["path"].to_list():
        Path(path).unlink()
    for sym_dir in args.data_dir.iterdir():
        if sym_dir.is_dir() and not any(sym_dir.iterdir()):
            sym_dir.rmdir()

    # Append-only deletion log; later runs no longer see deleted files
    log_path = args.manifest_dir / "pruned.csv"
    entries = plan.with_columns(pl.lit(datetime.now(UTC).strftime("%Y-%m-%d")).alias("pruned_on"))
    if log_path.exists():
        entries = pl.concat([pl.read_csv(log_path, infer_schema=False), entries])
    entries.write_csv(log_path)
    print(f"deleted {pruned.height} files (logged to {log_path})")


if __name__ == "__main__":
    main()
