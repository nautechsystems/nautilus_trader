"""
Download Binance USD-M futures metrics from data.binance.vision.

Dataset: metrics (daily files, 5-minute rows): open interest (contracts and value), top-trader
and global long/short ratios, and the taker long/short volume ratio.

Rules:
- Symbols: the kept symbols of the kline manifest (`data/binance/manifest/symbols.csv`).
- Months from --start to --end inclusive; months at or after SEAL_MONTH are refused, so sealed
  data never reaches the disk.
- Every archive is checked against its published SHA-256 checksum before it is parsed; a
  mismatch or a missing checksum aborts the run.
- Output: one parquet per symbol and month, written only after every file of that month was
  downloaded and verified, so an interrupted run resumes by skipping finished months.
  `time` is the 5-minute `create_time` in epoch milliseconds (UTC); a row stamped t covers
  [t, t + 5 min) and holds the open interest at t + 5 min. Empty ratio fields (early 2022)
  become nulls.
- A log of every written month (files, rows, missing days) is appended to
  `data/binance/manifest/derivs_downloads.csv`.

Usage (from the repository root):
    python/.venv/Scripts/python research/binance_derivs.py
    python/.venv/Scripts/python research/binance_derivs.py --symbols BTCUSDT --start 2024-01

"""

from __future__ import annotations

import argparse
import calendar
import hashlib
import io
import re
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
import zipfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import polars as pl


ROOT = Path(__file__).resolve().parents[1]
DATA_DIR = ROOT / "data" / "binance"
MANIFEST = DATA_DIR / "manifest" / "symbols.csv"
LOG = DATA_DIR / "manifest" / "derivs_downloads.csv"
OUT_DIR = DATA_DIR / "futures_metrics"
LISTING_URL = "https://s3-ap-northeast-1.amazonaws.com/data.binance.vision"
FILES_URL = "https://data.binance.vision"
PREFIX = "data/futures/um/daily/metrics/{symbol}/"
PATTERN = re.compile(r"-metrics-(\d{4})-(\d{2})-(\d{2})\.zip$")
SEAL_MONTH = (2026, 7)
RETRIES = 4
METRIC_COLS = (
    "sum_open_interest",
    "sum_open_interest_value",
    "count_toptrader_long_short_ratio",
    "sum_toptrader_long_short_ratio",
    "count_long_short_ratio",
    "sum_taker_long_short_vol_ratio",
)


def fetch(url: str) -> bytes:
    for attempt in range(RETRIES):
        try:
            with urllib.request.urlopen(url, timeout=60) as resp:
                return resp.read()
        except urllib.error.HTTPError:
            raise
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            if attempt == RETRIES - 1:
                raise
            time.sleep(2**attempt)
    raise AssertionError("unreachable")


def list_keys(prefix: str) -> list[str]:
    """
    Every object key under `prefix` (the S3 listing is paginated at 1000 keys).
    """
    ns = {"s3": "http://s3.amazonaws.com/doc/2006-03-01/"}
    keys: list[str] = []
    marker = ""
    while True:
        query = urllib.parse.urlencode({"prefix": prefix, "marker": marker})
        root = ET.fromstring(fetch(f"{LISTING_URL}?{query}"))
        page = [k.text for k in root.findall("s3:Contents/s3:Key", ns) if k.text]
        keys.extend(page)
        if root.findtext("s3:IsTruncated", default="false", namespaces=ns) != "true" or not page:
            return keys
        marker = page[-1]


def download_verified(key: str) -> bytes:
    data = fetch(f"{FILES_URL}/{key}")
    expected = fetch(f"{FILES_URL}/{key}.CHECKSUM").decode().split()[0].lower()
    actual = hashlib.sha256(data).hexdigest()
    if actual != expected:
        raise ValueError(f"checksum mismatch for {key}")
    return data


def read_zip_csv(data: bytes) -> pl.DataFrame:
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        names = [n for n in z.namelist() if n.endswith(".csv")]
        if len(names) != 1:
            raise ValueError(f"expected one CSV in the archive, found {names}")
        return pl.read_csv(z.read(names[0]), infer_schema=False)


def parse_metrics(raw: pl.DataFrame) -> pl.DataFrame:
    return raw.select(
        pl.col("create_time")
        .str.to_datetime("%Y-%m-%d %H:%M:%S", time_zone="UTC")
        .dt.epoch("ms")
        .alias("time"),
        *[pl.col(c).replace("", None).cast(pl.Float64) for c in METRIC_COLS],
    )


def months_in_range(start: tuple[int, int], end: tuple[int, int]) -> list[tuple[int, int]]:
    out = []
    y, m = start
    while (y, m) <= end:
        out.append((y, m))
        y, m = (y + 1, 1) if m == 12 else (y, m + 1)
    return out


def run_symbol(symbol: str, months: set[tuple[int, int]], pool: ThreadPoolExecutor) -> list[dict]:
    out_dir = OUT_DIR / symbol
    by_month: dict[tuple[int, int], list[tuple[str, int]]] = {}
    for key in list_keys(PREFIX.format(symbol=symbol)):
        m = PATTERN.search(key)
        if m is None:
            continue
        month = (int(m[1]), int(m[2]))
        if month in months:
            by_month.setdefault(month, []).append((key, int(m[3])))

    def path_of(y: int, mo: int) -> Path:
        return out_dir / f"{symbol}-metrics-{y}-{mo:02d}.parquet"

    todo = {
        month: sorted(files)
        for month, files in by_month.items()
        if not path_of(*month).exists()
    }
    if any(month >= SEAL_MONTH for month in todo):
        raise RuntimeError(f"refusing sealed months for {symbol}")
    keys = [k for files in todo.values() for k, _ in files]
    data = dict(zip(keys, pool.map(download_verified, keys), strict=True))

    log = []
    for (y, mo), files in sorted(todo.items()):
        path = path_of(y, mo)
        df = (
            pl.concat([parse_metrics(read_zip_csv(data[k])) for k, _ in files])
            .sort("time")
            .unique("time", keep="first", maintain_order=True)
        )
        out_dir.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(".tmp")
        df.write_parquet(tmp)
        tmp.replace(path)
        log.append(
            {
                "dataset": "metrics",
                "symbol": symbol,
                "month": f"{y}-{mo:02d}",
                "files": len(files),
                "rows": df.height,
                "missing_days": calendar.monthrange(y, mo)[1] - len({d for _, d in files}),
            }
        )
    return log


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[1])
    parser.add_argument("--symbols", nargs="+", default=None)
    parser.add_argument("--start", default="2022-01")
    parser.add_argument("--end", default="2026-06")
    parser.add_argument("--workers", type=int, default=48)
    args = parser.parse_args()

    start = tuple(int(x) for x in args.start.split("-"))
    end = tuple(int(x) for x in args.end.split("-"))
    if end >= SEAL_MONTH:
        raise SystemExit(f"--end {args.end} reaches the sealed period")
    months = set(months_in_range(start, end))
    symbols = args.symbols or (
        pl.read_csv(MANIFEST).filter(pl.col("kept"))["symbol"].sort().to_list()
    )
    started = time.time()
    with ThreadPoolExecutor(args.workers) as pool:
        for i, symbol in enumerate(symbols, 1):
            log = run_symbol(symbol, months, pool)
            if log:
                frame = pl.DataFrame(log)
                if LOG.exists():
                    frame = pl.concat([pl.read_csv(LOG), frame], how="vertical_relaxed")
                frame.write_csv(LOG)
            rows = sum(r["rows"] for r in log)
            print(
                f"{i}/{len(symbols)} {symbol}: {len(log)} months, {rows} rows "
                f"({time.time() - started:.0f}s)",
                flush=True,
            )


if __name__ == "__main__":
    main()
