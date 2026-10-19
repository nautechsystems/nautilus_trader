"""
Build 1-second bars from Bitget's free USDT-M perpetual trade archive.

Bitget publishes no 1-second candles (its finest is 1 minute), but its history-data page
(`/v1/statistics/public/download/getPublicDataV2`) gives every fill. The trades of each UTC day
are downloaded, verified, aggregated into 1-second bars and discarded; only the bars are
written. From mid 2023 Bitget stamps trades in whole seconds (before that in milliseconds), so
one second is the finest grid that is uniform over the whole history.

Rules:
- Symbols: the 13 coins of the 1-minute research universe, under their Bitget names; Binance's 1000SHIBUSDT is Bitget's SHIBUSDT (price per 1 SHIB).
- Days from --start to --end inclusive are UTC days. Bitget files are UTC+8 days (the file
  labelled D covers D-1 16:00 UTC to D 16:00 UTC), so a UTC day is cut from the two labels that
  cover it and is written only when both are complete.
- Sealed data (2026-07-01 00:00 UTC on) never reaches the disk: --end must be before SEAL_DAY,
  a label whose coverage reaches past it is never downloaded (so UTC day 2026-06-30, which would
  need the label 2026-07-01, is never written), and trades at or after it are dropped.
- A label is complete when its part files are numbered 1..N without a hole and every part
  downloads and passes its zip CRC. A day with an incomplete label is reported, logged as
  missing and left unwritten.
- Each day is written via a temporary name, so an interrupted run resumes by skipping finished
  days.

Output: data/bitget/bars_1s/<SYMBOL>/<SYMBOL>-bars_1s-YYYY-MM-DD.parquet, one row
per second with at least one trade (seconds without trades have no row):
- time         bar start, epoch milliseconds (UTC); the bar covers [time, time + 1000)
- open, high, low, close   trade prices in file order within the second (Bitget's own 1m
                           candles instead open at the previous close)
- buy_qty, sell_qty        base-coin quantity by taker side
- buy_quote, sell_quote    USDT notional (price * qty) by taker side
- trades                   number of fills
`data/bitget/manifest/downloads.csv` logs every day (status, bars, trades, span).

Usage (from the repository root):
    python/.venv/Scripts/python research/bitget_bars.py
    python/.venv/Scripts/python research/bitget_bars.py --symbols BTCUSDT --start 2026-06-01 --end 2026-06-07

"""

from __future__ import annotations

import argparse
import http.client
import io
import json
import re
import time
import urllib.error
import urllib.request
import zipfile
from concurrent.futures import ThreadPoolExecutor
from datetime import UTC
from datetime import date
from datetime import datetime
from datetime import timedelta
from pathlib import Path

import polars as pl


ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = ROOT / "data" / "bitget" / "bars_1s"
LOG = ROOT / "data" / "bitget" / "manifest" / "downloads.csv"
SYMBOLS = (
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
    "AVAXUSDT", "NEARUSDT", "SHIBUSDT", "GALAUSDT", "MANAUSDT", "SANDUSDT",
)  # fmt: skip
SEAL_DAY = date(2026, 7, 1)
DAY_MS = 86_400_000
UTC8_MS = 8 * 3_600_000
RETRIES = 6
RATE_LIMIT_WAIT = 10  # seconds, times the attempt number, after an HTTP 429
CHUNK_DAYS = 7
LIST_URL = "https://www.bitget.com/v1/statistics/public/download/getPublicDataV2"
LIST_DAYS = 7  # the page's own limit for one query
PART = re.compile(r"[/_](\d{8})_(\d{3})\.zip$")  # .../YYYYMMDD_NNN.zip, older ..._YYYYMMDD_NNN.zip


def fetch(url: str, body: bytes | None = None) -> bytes | None:
    """
    The body at `url` (POSTing `body` as JSON when given), or None when the server answers 404.
    """
    headers = {"User-Agent": "Mozilla/5.0"}
    if body is not None:
        headers["Content-Type"] = "application/json"
    for attempt in range(RETRIES):
        try:
            req = urllib.request.Request(url, data=body, headers=headers)
            with urllib.request.urlopen(req, timeout=120) as resp:
                return resp.read()
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return None
            retryable = e.code == 429 or e.code >= 500
            if not retryable or attempt == RETRIES - 1:
                raise
            if e.code == 429:
                time.sleep(RATE_LIMIT_WAIT * (attempt + 1))
                continue
        except (urllib.error.URLError, http.client.IncompleteRead, TimeoutError, ConnectionError):
            if attempt == RETRIES - 1:
                raise
        time.sleep(2**attempt)
    raise AssertionError("unreachable")


def day_ms(d: date) -> int:
    return int(datetime(d.year, d.month, d.day, tzinfo=UTC).timestamp() * 1000)


def days_in_range(start: date, end: date) -> list[date]:
    return [start + timedelta(days=i) for i in range((end - start).days + 1)]


SEAL_MS = day_ms(SEAL_DAY)


def label_cover(label: date) -> tuple[int, int]:
    start = day_ms(label) - UTC8_MS
    return start, start + DAY_MS


def list_parts(symbol: str, labels: list[date]) -> dict[date, list[str] | None]:
    """
    {label: part URLs in order, or None when the parts are absent or not numbered 1..N}.
    """
    found: dict[date, dict[int, str]] = {}
    lo = labels[0]
    while lo <= labels[-1]:
        hi = min(lo + timedelta(days=LIST_DAYS - 1), labels[-1])
        payload = {
            "displaySymbol": [symbol],
            "businessLine": 2,  # futures
            "businessType": 2,  # trades
            "dateType": 1,  # daily
            "beginTimeStr": lo.isoformat(),
            "endTimeStr": hi.isoformat(),
        }
        data = fetch(LIST_URL, json.dumps(payload).encode())
        if data is None:
            raise RuntimeError("Bitget listing returned 404")
        resp = json.loads(data)
        if resp.get("code") != "200":
            raise RuntimeError(f"Bitget listing error: {resp}")
        for item in resp["data"] or []:
            m = PART.search(item["fileUrl"])
            if m is None:
                raise ValueError(f"unexpected Bitget file URL {item['fileUrl']}")
            label = date(int(m[1][:4]), int(m[1][4:6]), int(m[1][6:]))
            found.setdefault(label, {})[int(m[2])] = item["fileUrl"]
        lo = hi + timedelta(days=1)
    out: dict[date, list[str] | None] = {}
    for label in labels:
        parts = found.get(label, {})
        complete = bool(parts) and sorted(parts) == list(range(1, len(parts) + 1))
        out[label] = [parts[i] for i in sorted(parts)] if complete else None
    return out


def load_part(url: str) -> pl.DataFrame | None:
    """
    One part file as time (epoch ms), price, qty, side (+1 taker buy, -1 taker sell); reading
    the archive checks its CRC.
    """
    data = fetch(url)
    if data is None:
        return None
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        names = [n for n in z.namelist() if n.endswith(".csv")]
        if len(names) != 1:
            raise ValueError(f"expected one CSV in {url}, found {names}")
        raw = pl.read_csv(z.read(names[0]), infer_schema=False)
    return raw.select(
        pl.col("timestamp").cast(pl.Int64).alias("time"),
        pl.col("price").cast(pl.Float64),
        pl.col("size(base)").cast(pl.Float64).alias("qty"),
        pl.when(pl.col("side").str.to_lowercase() == "buy")
        .then(1)
        .otherwise(-1)
        .cast(pl.Int8)
        .alias("side"),
    )


def to_bars(trades: pl.DataFrame) -> pl.DataFrame:
    buy = pl.col("side") == 1
    quote = pl.col("price") * pl.col("qty")
    return (
        trades.group_by((pl.col("time") // 1000 * 1000).alias("time"), maintain_order=True)
        .agg(
            pl.col("price").first().alias("open"),
            pl.col("price").max().alias("high"),
            pl.col("price").min().alias("low"),
            pl.col("price").last().alias("close"),
            pl.col("qty").filter(buy).sum().alias("buy_qty"),
            pl.col("qty").filter(~buy).sum().alias("sell_qty"),
            quote.filter(buy).sum().alias("buy_quote"),
            quote.filter(~buy).sum().alias("sell_quote"),
            pl.len().cast(pl.Int64).alias("trades"),
        )
        .sort("time")
    )


def out_path(symbol: str, d: date) -> Path:
    return OUT_DIR / symbol / f"{symbol}-bars_1s-{d.isoformat()}.parquet"


def log_row(symbol: str, d: date, status: str, **fields) -> dict:
    return {
        "symbol": symbol,
        "day": d.isoformat(),
        "status": status,
        "bars": fields.get("bars"),
        "trades": fields.get("trades"),
        "first": fields.get("first"),
        "last": fields.get("last"),
    }


def run(symbol: str, days: list[date], pool: ThreadPoolExecutor) -> list[dict]:
    todo = [d for d in days if not out_path(symbol, d).exists()]
    log: list[dict] = []
    carry: dict[date, pl.DataFrame | None] = {}
    for i in range(0, len(todo), CHUNK_DAYS):
        chunk = todo[i : i + CHUNK_DAYS]
        plan: dict[date, tuple[date, date]] = {}
        for d in chunk:
            labels = (d, d + timedelta(days=1))
            if label_cover(labels[1])[1] > SEAL_MS:
                print(f"  {symbol} {d}: needs a sealed source file, skipped")
                continue
            plan[d] = labels
        needed = sorted({lb for labels in plan.values() for lb in labels} - set(carry))
        frames: dict[date, pl.DataFrame | None] = dict(carry)
        if needed:
            listing = list_parts(symbol, needed)
            urls = [u for lb in needed for u in listing[lb] or []]
            parts = dict(zip(urls, pool.map(load_part, urls), strict=True))
            for lb in needed:
                got = [parts[u] for u in listing[lb]] if listing[lb] is not None else None
                frames[lb] = None if got is None or any(p is None for p in got) else pl.concat(got)
        for d, labels in plan.items():
            if any(frames[lb] is None for lb in labels):
                absent = [lb.isoformat() for lb in labels if frames[lb] is None]
                print(f"  {symbol} {d}: incomplete upstream label {absent}")
                log.append(log_row(symbol, d, "missing"))
                continue
            lo, hi = day_ms(d), min(day_ms(d) + DAY_MS, SEAL_MS)
            trades = (
                pl.concat([frames[lb] for lb in labels])
                .filter((pl.col("time") >= lo) & (pl.col("time") < hi))
                .sort("time", maintain_order=True)
            )
            bars = to_bars(trades)
            path = out_path(symbol, d)
            path.parent.mkdir(parents=True, exist_ok=True)
            tmp = path.with_suffix(".tmp")
            bars.write_parquet(tmp)
            tmp.replace(path)
            log.append(
                log_row(
                    symbol,
                    d,
                    "ok",
                    bars=bars.height,
                    trades=trades.height,
                    first=trades["time"].min(),
                    last=trades["time"].max(),
                )
            )
        # Day D+1 reuses the label D+1 that day D already needed.
        carry = {plan[max(plan)][1]: frames[plan[max(plan)][1]]} if plan else {}
    return log


def append_log(rows: list[dict]) -> None:
    if not rows:
        return
    schema = {
        "symbol": pl.String,
        "day": pl.String,
        "status": pl.String,
        "bars": pl.Int64,
        "trades": pl.Int64,
        "first": pl.Int64,
        "last": pl.Int64,
    }
    frame = pl.DataFrame(rows, schema=schema)
    LOG.parent.mkdir(parents=True, exist_ok=True)
    if LOG.exists():
        frame = pl.concat([pl.read_csv(LOG, schema=schema), frame])
    frame.write_csv(LOG)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[1])
    parser.add_argument("--symbols", nargs="+", default=list(SYMBOLS))
    parser.add_argument("--start", default="2022-01-01", help="first UTC day, YYYY-MM-DD")
    parser.add_argument("--end", default="2026-06-29", help="last UTC day, YYYY-MM-DD")
    parser.add_argument("--workers", type=int, default=16)
    args = parser.parse_args()

    start, end = date.fromisoformat(args.start), date.fromisoformat(args.end)
    if end >= SEAL_DAY:
        raise SystemExit(f"--end {end} reaches the sealed period (from {SEAL_DAY})")
    days = days_in_range(start, end)
    started = time.time()
    with ThreadPoolExecutor(args.workers) as pool:
        for i, symbol in enumerate(args.symbols, 1):
            log = run(symbol, days, pool)
            append_log(log)
            ok = sum(r["status"] == "ok" for r in log)
            missing = sum(r["status"] == "missing" for r in log)
            print(
                f"{i}/{len(args.symbols)} {symbol}: {ok} days written, {missing} missing "
                f"upstream ({time.time() - started:.0f}s)",
                flush=True,
            )


if __name__ == "__main__":
    main()
