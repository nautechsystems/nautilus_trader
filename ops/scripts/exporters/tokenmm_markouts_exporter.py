#!/usr/bin/env python3
"""Prometheus sidecar exporter for durable TokenMM markout aggregates."""

from __future__ import annotations

import argparse
import functools
import logging
import re
import signal
import sqlite3
import sys
import time
from pathlib import Path
from typing import Any

import pandas as pd
from prometheus_client import CollectorRegistry
from prometheus_client import Gauge
from prometheus_client import start_http_server

REPO_ROOT = Path(__file__).resolve().parents[3]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from ops.scripts.exporters.common import poll_interval_seconds_arg
from ops.scripts.exporters.common import positive_float_arg
from research.tokenmm.telemetry_helpers import merge_fills_and_markouts
from research.tokenmm.telemetry_helpers import load_sqlite_query
from research.tokenmm.telemetry_helpers import numeric


LOGGER = logging.getLogger("tokenmm_markouts_exporter")

LABEL_NAMES = (
    "env",
    "profile",
    "strategy_id",
    "venue",
    "symbol",
    "order_side",
    "horizon_s",
    "benchmark_name",
    "analysis_window",
)
ANALYSIS_WINDOWS = (
    ("15m", 0.25),
    ("1h", 1.0),
    ("4h", 4.0),
    ("24h", 24.0),
)
MAX_ANALYSIS_WINDOW_HOURS = max(hours for _label, hours in ANALYSIS_WINDOWS)
DEFAULT_TELEMETRY_ROOT = Path("/var/lib/nautilus/telemetry")
DEFAULT_BENCHMARK_NAME = "fv_market_mid"
PROFILE_NORMALIZER = re.compile(r"[^a-z0-9]+")
MARKOUT_QUERY_COLUMNS = (
    "trader_id",
    "event_id",
    "strategy_id",
    "benchmark_name",
    "horizon_s",
    "target_ts_ms",
    "markout_bps",
    "fill_px",
    "fill_qty",
    "resolution_status",
)
LEGACY_FILL_QUERY_COLUMNS = (
    "trader_id",
    "event_id",
    "strategy_id",
    "order_side",
    "instrument_id",
    "fill_px",
    "fill_qty",
    "fill_ts_ms",
)
LIVE_FILL_QUERY_COLUMNS = (
    "trader_id",
    "event_id",
    "strategy_id",
    "order_side",
    "instrument_id",
    "last_px AS fill_px",
    "last_qty AS fill_qty",
    "CAST(ts_event / 1000000 AS INTEGER) AS fill_ts_ms",
)
LIVE_NORMALIZED_FILL_QUERY_COLUMNS = (
    "trader_id",
    "event_id",
    "strategy_id",
    "order_side",
    "instrument_id",
    "last_px AS fill_px",
    "COALESCE(last_qty_base, last_qty) AS fill_qty",
    "last_qty_base AS fill_qty_base",
    "COALESCE(last_qty_venue, last_qty) AS fill_qty_venue",
    "CAST(ts_event / 1000000 AS INTEGER) AS fill_ts_ms",
)
FILL_QUERY_COLUMNS = LEGACY_FILL_QUERY_COLUMNS


def normalize_profile(profile: str) -> str:
    text = PROFILE_NORMALIZER.sub("_", str(profile or "").strip().lower()).strip("_")
    return text or "tokenmm"


def normalize_benchmark_names(raw_value: Any) -> tuple[str, ...]:
    if isinstance(raw_value, str):
        raw_items = raw_value.split(",")
    elif isinstance(raw_value, (list, tuple, set)):
        raw_items = list(raw_value)
    elif raw_value is None:
        raw_items = [DEFAULT_BENCHMARK_NAME]
    else:
        raw_items = [raw_value]

    benchmark_names: list[str] = []
    seen: set[str] = set()
    for raw_item in raw_items:
        benchmark_name = str(raw_item).strip()
        if not benchmark_name or benchmark_name in seen:
            continue
        seen.add(benchmark_name)
        benchmark_names.append(benchmark_name)
    return tuple(benchmark_names or [DEFAULT_BENCHMARK_NAME])


def analysis_window_hours_arg(raw_value: str) -> float:
    value = positive_float_arg(raw_value)
    if value < MAX_ANALYSIS_WINDOW_HOURS:
        raise argparse.ArgumentTypeError(
            "must be >= the largest supported analysis window "
            f"({MAX_ANALYSIS_WINDOW_HOURS:g}h)",
        )
    return value

def default_db_paths(
    *,
    profile: str,
    telemetry_dir: Path | str | None = None,
) -> dict[str, Path]:
    if telemetry_dir is None:
        base_dir = DEFAULT_TELEMETRY_ROOT / normalize_profile(profile)
    else:
        base_dir = Path(telemetry_dir)
    return {
        "fills": base_dir / "fills.sqlite",
        "markouts": base_dir / "markouts.sqlite",
    }


def _sql_literal(value: Any) -> str:
    return "'" + str(value).replace("'", "''") + "'"


def _build_markouts_query(
    *,
    benchmark_name: str | None = None,
    benchmark_names: tuple[str, ...] | None = None,
    window_hours: float,
    now_ms: int,
) -> str:
    normalized_benchmark_names = (
        benchmark_names
        if benchmark_names is not None
        else normalize_benchmark_names(benchmark_name)
    )
    if len(normalized_benchmark_names) == 1:
        filters = [f"benchmark_name = {_sql_literal(normalized_benchmark_names[0])}"]
    else:
        filters = [
            "benchmark_name IN ({values})".format(
                values=", ".join(_sql_literal(value) for value in normalized_benchmark_names),
            ),
        ]
    if window_hours > 0:
        window_start_ms = now_ms - int(window_hours * 60 * 60 * 1000)
        filters.append(f"target_ts_ms BETWEEN {window_start_ms} AND {now_ms}")
    where_clause = " AND ".join(filters)
    select_cols = ", ".join(MARKOUT_QUERY_COLUMNS)
    return (
        f"SELECT {select_cols} FROM execution_markout "
        f"WHERE {where_clause} "
        "ORDER BY target_ts_ms DESC"
    )


@functools.lru_cache(maxsize=8)
def _table_columns(db_path: str, table: str) -> tuple[str, ...]:
    path = Path(db_path)
    if not path.exists():
        raise FileNotFoundError(path)
    with sqlite3.connect(path) as conn:
        rows = conn.execute(f"PRAGMA table_info({table})").fetchall()
    return tuple(str(row[1]) for row in rows if len(row) >= 2)


def _fill_query_columns_for_path(fills_path: Path) -> tuple[str, ...]:
    try:
        columns = set(_table_columns(str(Path(fills_path)), "execution_fill"))
    except FileNotFoundError:
        return LEGACY_FILL_QUERY_COLUMNS
    if {
        "trader_id",
        "event_id",
        "strategy_id",
        "order_side",
        "instrument_id",
        "fill_px",
        "fill_qty",
        "fill_ts_ms",
    }.issubset(columns):
        return LEGACY_FILL_QUERY_COLUMNS
    if {
        "trader_id",
        "event_id",
        "strategy_id",
        "order_side",
        "instrument_id",
        "last_px",
        "last_qty",
        "last_qty_base",
        "last_qty_venue",
        "ts_event",
    }.issubset(columns):
        return LIVE_NORMALIZED_FILL_QUERY_COLUMNS
    if {
        "trader_id",
        "event_id",
        "strategy_id",
        "order_side",
        "instrument_id",
        "last_px",
        "last_qty",
        "ts_event",
    }.issubset(columns):
        return LIVE_FILL_QUERY_COLUMNS
    raise ValueError(
        f"execution_fill schema missing compatible columns in {fills_path}",
    )


def _build_fills_query(
    markouts: list[dict[str, Any]],
    *,
    select_columns: tuple[str, ...] = FILL_QUERY_COLUMNS,
) -> str | None:
    fill_keys: list[tuple[str, str]] = []
    seen: set[tuple[str, str]] = set()
    for row in markouts:
        trader_id = row.get("trader_id")
        event_id = row.get("event_id")
        if trader_id is None or event_id is None:
            continue
        fill_key = (str(trader_id), str(event_id))
        if fill_key in seen:
            continue
        seen.add(fill_key)
        fill_keys.append(fill_key)
    if not fill_keys:
        return None

    select_cols = ", ".join(select_columns)
    row_values = ", ".join(
        "({trader_id}, {event_id})".format(
            trader_id=_sql_literal(trader_id),
            event_id=_sql_literal(event_id),
        )
        for trader_id, event_id in fill_keys
    )
    return (
        f"SELECT {select_cols} FROM execution_fill "
        f"WHERE (trader_id, event_id) IN ({row_values})"
    )


def load_markout_snapshot(
    *,
    fills_path: Path,
    markouts_path: Path,
    benchmark_name: str = DEFAULT_BENCHMARK_NAME,
    benchmark_names: tuple[str, ...] | None = None,
    window_hours: float = 24.0,
    now_ms: int | None = None,
) -> list[dict[str, Any]]:
    merged = _load_merged_markout_dataset(
        fills_path=fills_path,
        markouts_path=markouts_path,
        benchmark_name=benchmark_name,
        benchmark_names=benchmark_names,
        window_hours=window_hours,
        now_ms=now_ms,
    )
    return _build_markout_snapshot_rows(
        merged=merged,
        window_hours=window_hours,
        now_ms=now_ms,
    )


def _load_merged_markout_dataset(
    *,
    fills_path: Path,
    markouts_path: Path,
    benchmark_name: str = DEFAULT_BENCHMARK_NAME,
    benchmark_names: tuple[str, ...] | None = None,
    window_hours: float = 24.0,
    now_ms: int | None = None,
) -> pd.DataFrame:
    if float(window_hours) <= 0:
        raise ValueError("window_hours must be > 0 for bounded polling")
    if now_ms is None:
        now_ms = int(time.time() * 1000)

    markouts_query = _build_markouts_query(
        benchmark_name=benchmark_name,
        benchmark_names=benchmark_names,
        window_hours=window_hours,
        now_ms=now_ms,
    )
    markouts = load_sqlite_query(markouts_path, markouts_query)
    if markouts.empty:
        return pd.DataFrame()

    fills_query = _build_fills_query(
        markouts.to_dict("records"),
        select_columns=_fill_query_columns_for_path(fills_path),
    )
    if not fills_query:
        return pd.DataFrame()
    fills = load_sqlite_query(fills_path, fills_query)
    merged = merge_fills_and_markouts(fills=fills, markouts=markouts)
    if merged.empty:
        return pd.DataFrame()

    merged["venue"] = merged["venue"].fillna("unknown").astype(str)
    merged["symbol"] = merged["symbol"].fillna("UNKNOWN").astype(str)
    merged["order_side"] = merged["order_side"].fillna("UNKNOWN").astype(str).str.upper()
    normalized_benchmark_names = (
        benchmark_names
        if benchmark_names is not None
        else normalize_benchmark_names(benchmark_name)
    )
    merged["benchmark_name"] = (
        merged["benchmark_name"].fillna(normalized_benchmark_names[0]).astype(str)
    )
    merged["resolution_status"] = merged["resolution_status"].fillna("unknown").astype(str)
    merged["horizon_s"] = numeric(merged["horizon_s"]).astype("Int64")
    merged["markout_bps_num"] = numeric(merged["markout_bps_num"])
    merged["fill_notional_num"] = numeric(merged["fill_notional"]).fillna(0.0)
    merged["target_ts_ms_num"] = numeric(merged["target_ts_ms"])
    return merged


def _build_markout_snapshot_rows(
    *,
    merged: pd.DataFrame,
    window_hours: float = 24.0,
    now_ms: int | None = None,
) -> list[dict[str, Any]]:
    if float(window_hours) <= 0:
        raise ValueError("window_hours must be > 0 for bounded polling")
    if now_ms is None:
        now_ms = int(time.time() * 1000)
    if merged.empty:
        return []
    scoped = merged
    if "target_ts_ms_num" in merged.columns:
        window_start_ms = now_ms - int(window_hours * 60 * 60 * 1000)
        scoped = merged.loc[
            merged["target_ts_ms_num"].between(window_start_ms, now_ms, inclusive="both")
        ].copy()
    if scoped.empty:
        return []
    rows: list[dict[str, Any]] = []
    group_cols = ["strategy_id", "venue", "symbol", "order_side", "horizon_s", "benchmark_name"]
    for group_key, group in scoped.groupby(group_cols, dropna=False, sort=True):
        if not isinstance(group_key, tuple):
            group_key = (group_key,)
        strategy_id, venue, symbol, order_side, horizon_s, benchmark_name_value = group_key
        resolved = group.loc[
            (group["resolution_status"] == "resolved")
            & group["markout_bps_num"].notna()
        ].copy()
        fill_count = int(group["fill_key"].nunique()) if "fill_key" in group.columns else int(len(group))
        total_rows = int(len(group))
        resolved_rows = int(len(resolved))
        resolution_rate = float(resolved_rows / total_rows) if total_rows else 0.0
        avg_bps = float(resolved["markout_bps_num"].mean()) if resolved_rows else None
        nw_bps = None
        if resolved_rows:
            weights = resolved["fill_notional_num"]
            if (weights > 0).any():
                nw_bps = float((resolved["markout_bps_num"] * weights).sum() / weights.sum())
        last_target_ts_seconds = None
        if group["target_ts_ms_num"].notna().any():
            last_target_ts_seconds = float(group["target_ts_ms_num"].max() / 1000.0)
        rows.append(
            {
                "strategy_id": str(strategy_id),
                "venue": str(venue),
                "symbol": str(symbol),
                "order_side": str(order_side),
                "horizon_s": str(int(horizon_s)),
                "benchmark_name": str(benchmark_name_value),
                "fill_count": fill_count,
                "resolved_rows": resolved_rows,
                "resolution_rate": resolution_rate,
                "avg_bps": avg_bps,
                "nw_bps": nw_bps,
                "last_target_ts_seconds": last_target_ts_seconds,
            },
        )
    return rows


class TokenMMMarkoutsExporter:
    def __init__(
        self,
        *,
        fills_path: Path,
        markouts_path: Path,
        env: str,
        profile: str,
        window_hours: float = 24.0,
        benchmark_name: str = DEFAULT_BENCHMARK_NAME,
        registry: CollectorRegistry | None = None,
    ) -> None:
        if float(window_hours) <= 0:
            raise ValueError("window_hours must be > 0 for bounded polling")
        self.fills_path = Path(fills_path)
        self.markouts_path = Path(markouts_path)
        self.env = str(env or "prod")
        self.profile = normalize_profile(profile)
        self.window_hours = float(window_hours)
        self.benchmark_names = normalize_benchmark_names(benchmark_name)
        if self.window_hours < MAX_ANALYSIS_WINDOW_HOURS:
            raise ValueError(
                "window_hours must be >= the maximum supported analysis window "
                f"({MAX_ANALYSIS_WINDOW_HOURS:g}h)",
            )
        self.analysis_windows = ANALYSIS_WINDOWS
        self.registry = registry or CollectorRegistry(auto_describe=True)
        self.g_avg_bps = Gauge(
            "tokenmm_markout_avg_bps",
            "Average resolved markout in basis points by label tuple",
            LABEL_NAMES,
            registry=self.registry,
        )
        self.g_nw_bps = Gauge(
            "tokenmm_markout_nw_bps",
            "Notional-weighted resolved markout in basis points by label tuple",
            LABEL_NAMES,
            registry=self.registry,
        )
        self.g_resolved_rows = Gauge(
            "tokenmm_markout_resolved_rows",
            "Resolved markout row count by label tuple",
            LABEL_NAMES,
            registry=self.registry,
        )
        self.g_fill_count = Gauge(
            "tokenmm_markout_fill_count",
            "Unique fill count contributing to the label tuple",
            LABEL_NAMES,
            registry=self.registry,
        )
        self.g_resolution_rate = Gauge(
            "tokenmm_markout_resolution_rate",
            "Resolution rate for the label tuple",
            LABEL_NAMES,
            registry=self.registry,
        )
        self.g_last_target_ts_seconds = Gauge(
            "tokenmm_markout_last_target_ts_seconds",
            "Most recent markout target timestamp for the label tuple",
            LABEL_NAMES,
            registry=self.registry,
        )
        self._active_labels: dict[str, set[tuple[str, ...]]] = {
            "avg_bps": set(),
            "nw_bps": set(),
            "resolved_rows": set(),
            "fill_count": set(),
            "resolution_rate": set(),
            "last_target_ts_seconds": set(),
        }

    def _labels(self, row: dict[str, Any], *, analysis_window: str) -> dict[str, str]:
        return {
            "env": self.env,
            "profile": self.profile,
            "strategy_id": str(row["strategy_id"]),
            "venue": str(row["venue"]),
            "symbol": str(row["symbol"]),
            "order_side": str(row["order_side"]),
            "horizon_s": str(row["horizon_s"]),
            "benchmark_name": str(row["benchmark_name"]),
            "analysis_window": str(analysis_window),
        }

    def _label_values(self, labels: dict[str, str]) -> tuple[str, ...]:
        return tuple(labels[name] for name in LABEL_NAMES)

    def _sync_metric(
        self,
        *,
        gauge: Gauge,
        metric_key: str,
        values: dict[tuple[str, ...], float],
    ) -> None:
        previous = self._active_labels[metric_key]
        for label_values in previous - set(values):
            gauge.remove(*label_values)
        for label_values, value in values.items():
            gauge.labels(*label_values).set(value)
        self._active_labels[metric_key] = set(values)

    def poll_once(self, *, now_ms: int | None = None) -> None:
        avg_values: dict[tuple[str, ...], float] = {}
        nw_values: dict[tuple[str, ...], float] = {}
        resolved_values: dict[tuple[str, ...], float] = {}
        fill_values: dict[tuple[str, ...], float] = {}
        resolution_values: dict[tuple[str, ...], float] = {}
        last_target_values: dict[tuple[str, ...], float] = {}
        merged = _load_merged_markout_dataset(
            fills_path=self.fills_path,
            markouts_path=self.markouts_path,
            benchmark_name=self.benchmark_names[0],
            benchmark_names=self.benchmark_names,
            window_hours=self.window_hours,
            now_ms=now_ms,
        )

        for analysis_window, window_hours in self.analysis_windows:
            snapshot = _build_markout_snapshot_rows(
                merged=merged,
                window_hours=window_hours,
                now_ms=now_ms,
            )
            for row in snapshot:
                labels = self._labels(row, analysis_window=analysis_window)
                label_values = self._label_values(labels)
                if row["avg_bps"] is not None:
                    avg_values[label_values] = float(row["avg_bps"])
                if row["nw_bps"] is not None:
                    nw_values[label_values] = float(row["nw_bps"])
                resolved_values[label_values] = float(row["resolved_rows"])
                fill_values[label_values] = float(row["fill_count"])
                resolution_values[label_values] = float(row["resolution_rate"])
                if row["last_target_ts_seconds"] is not None:
                    last_target_values[label_values] = float(row["last_target_ts_seconds"])

        self._sync_metric(gauge=self.g_avg_bps, metric_key="avg_bps", values=avg_values)
        self._sync_metric(gauge=self.g_nw_bps, metric_key="nw_bps", values=nw_values)
        self._sync_metric(
            gauge=self.g_resolved_rows,
            metric_key="resolved_rows",
            values=resolved_values,
        )
        self._sync_metric(gauge=self.g_fill_count, metric_key="fill_count", values=fill_values)
        self._sync_metric(
            gauge=self.g_resolution_rate,
            metric_key="resolution_rate",
            values=resolution_values,
        )
        self._sync_metric(
            gauge=self.g_last_target_ts_seconds,
            metric_key="last_target_ts_seconds",
            values=last_target_values,
        )


def _poll_once_with_logging(exporter: TokenMMMarkoutsExporter) -> None:
    try:
        exporter.poll_once()
    except Exception:
        LOGGER.exception(
            "markouts poll failed for fills_db=%s markouts_db=%s",
            getattr(exporter, "fills_path", "unknown"),
            getattr(exporter, "markouts_path", "unknown"),
        )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Export durable TokenMM markout aggregates as Prometheus gauges.",
    )
    parser.add_argument("--env", default="prod", help="Environment label for exported metrics.")
    parser.add_argument("--profile", default="tokenmm", help="Profile name for default DB paths.")
    parser.add_argument(
        "--telemetry-dir",
        help="Override telemetry directory containing fills.sqlite and markouts.sqlite.",
    )
    parser.add_argument("--fills-db", help="Override path to fills.sqlite.")
    parser.add_argument("--markouts-db", help="Override path to markouts.sqlite.")
    parser.add_argument(
        "--benchmark-name",
        default=DEFAULT_BENCHMARK_NAME,
        help="Benchmark name or comma-separated benchmark names to export.",
    )
    parser.add_argument(
        "--window-hours",
        type=analysis_window_hours_arg,
        default=24.0,
        help=(
            "Trailing target timestamp window used for bounded polling reads. "
            "Must cover the largest supported analysis window (currently 24h)."
        ),
    )
    parser.add_argument(
        "--port",
        type=int,
        default=9094,
        help="Port to bind the exporter HTTP server.",
    )
    parser.add_argument(
        "--poll-interval-s",
        type=poll_interval_seconds_arg,
        default=30.0,
        help="Polling interval in seconds.",
    )
    parser.add_argument(
        "--log-level",
        default="INFO",
        help="Python logging level.",
    )
    return parser


def _resolve_paths(args: argparse.Namespace) -> dict[str, Path]:
    defaults = default_db_paths(
        profile=args.profile,
        telemetry_dir=args.telemetry_dir,
    )
    return {
        "fills": Path(args.fills_db) if args.fills_db else defaults["fills"],
        "markouts": Path(args.markouts_db) if args.markouts_db else defaults["markouts"],
    }


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    logging.basicConfig(level=getattr(logging, str(args.log_level).upper(), logging.INFO))
    paths = _resolve_paths(args)
    exporter = TokenMMMarkoutsExporter(
        fills_path=paths["fills"],
        markouts_path=paths["markouts"],
        env=args.env,
        profile=args.profile,
        window_hours=args.window_hours,
        benchmark_name=args.benchmark_name,
    )

    start_http_server(int(args.port), registry=exporter.registry)
    LOGGER.info("tokenmm markouts exporter listening on :%s", args.port)

    done = False

    def _handle_signal(_signum: int, _frame: Any) -> None:
        nonlocal done
        done = True

    signal.signal(signal.SIGINT, _handle_signal)
    signal.signal(signal.SIGTERM, _handle_signal)

    while not done:
        _poll_once_with_logging(exporter)
        time.sleep(max(float(args.poll_interval_s), 0.5))

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
