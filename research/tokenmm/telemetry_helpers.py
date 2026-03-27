from __future__ import annotations

import json
import math
import re
import sqlite3
from bisect import bisect_left
from contextlib import suppress
from decimal import Decimal
from pathlib import Path
from typing import Any

import pandas as pd

from flux.persistence.markouts.common import signed_markout


FILL_ID_COLUMNS = ("trader_id", "event_id")
INSTRUMENT_PRODUCTS = ("SPOT", "LINEAR", "PERP", "SWAP")
NUMERIC_PATTERN = re.compile(r"[-+]?\d+(?:\.\d+)?")
AuditScalar = str | int | float | bool | None
AuditRow = dict[str, AuditScalar]


def load_sqlite_table(path: Path | str, table: str, limit: int | None = None) -> pd.DataFrame:
    db_path = Path(path)
    if not db_path.exists():
        raise FileNotFoundError(db_path)
    query = f"SELECT * FROM {table}"
    if limit is not None:
        query += f" ORDER BY rowid DESC LIMIT {int(limit)}"
    with sqlite3.connect(db_path) as conn:
        return pd.read_sql_query(query, conn)


def load_sqlite_query(path: Path | str, query: str) -> pd.DataFrame:
    db_path = Path(path)
    if not db_path.exists():
        raise FileNotFoundError(db_path)
    with sqlite3.connect(db_path) as conn:
        return pd.read_sql_query(query, conn)


def _json_mapping(value: Any) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    if isinstance(value, str) and value:
        with suppress(ValueError, json.JSONDecodeError):
            parsed = json.loads(value)
            if isinstance(parsed, dict):
                return parsed
    return {}


def numeric(series: pd.Series) -> pd.Series:
    if pd.api.types.is_numeric_dtype(series):
        return pd.to_numeric(series, errors="coerce")
    extracted = series.astype("string").str.extract(f"({NUMERIC_PATTERN.pattern})", expand=False)
    return pd.to_numeric(extracted, errors="coerce")


def ns_to_utc(series: pd.Series) -> pd.Series:
    return pd.to_datetime(pd.to_numeric(series, errors="coerce"), unit="ns", utc=True)


def ms_to_utc(series: pd.Series) -> pd.Series:
    return pd.to_datetime(pd.to_numeric(series, errors="coerce"), unit="ms", utc=True)


def parse_instrument_id(instrument_id: str | None) -> dict[str, str | None]:
    text = (instrument_id or "").strip()
    venue: str | None = None
    body = text
    if "." in text:
        body, venue = text.rsplit(".", 1)

    symbol = body
    product: str | None = None
    for candidate in INSTRUMENT_PRODUCTS:
        suffix = f"-{candidate}"
        if body.endswith(suffix):
            symbol = body[: -len(suffix)]
            product = candidate
            break

    return {"symbol": symbol or None, "venue": venue, "product": product}


def compute_fill_notional(
    price_or_frame: Any,
    qty: Any | None = None,
    *,
    qty_col: str = "last_qty",
    px_col: str = "last_px",
) -> float | pd.Series:
    if isinstance(price_or_frame, pd.DataFrame):
        qty_values = _resolve_fill_quantity_series(price_or_frame, qty_col=qty_col).abs()
        px_values = numeric(price_or_frame[px_col])
        return qty_values * px_values
    if qty is None:
        raise TypeError("qty is required when computing scalar notional")
    return float(abs(float(qty)) * float(price_or_frame))


def enrich_fills(fills: pd.DataFrame) -> pd.DataFrame:
    frame = fills.copy()
    if "fill_px_num" not in frame.columns:
        source = "last_px" if "last_px" in frame.columns else "fill_px"
        frame["fill_px_num"] = numeric(frame[source])
    else:
        frame["fill_px_num"] = numeric(frame["fill_px_num"])

    if "fill_qty_num" not in frame.columns:
        frame["fill_qty_num"] = _resolve_fill_quantity_series(frame)
    else:
        frame["fill_qty_num"] = numeric(frame["fill_qty_num"])

    frame["fill_qty_base_num"] = _resolve_fill_base_quantity_series(frame).fillna(frame["fill_qty_num"])
    frame["fill_qty_venue_num"] = _resolve_fill_venue_quantity_series(frame).fillna(frame["fill_qty_num"])

    if "fill_notional" not in frame.columns:
        if "notional" in frame.columns:
            frame["fill_notional"] = numeric(frame["notional"])
        else:
            frame["fill_notional"] = frame["fill_qty_num"].abs() * frame["fill_px_num"]
    else:
        frame["fill_notional"] = numeric(frame["fill_notional"])

    if "fill_ts_ms" not in frame.columns:
        if "ts_ms" in frame.columns:
            frame["fill_ts_ms"] = numeric(frame["ts_ms"]).astype("Int64")
        elif "ts_event" in frame.columns:
            ts_event_ns = numeric(frame["ts_event"])
            frame["fill_ts_ms"] = (ts_event_ns // 1_000_000).astype("Int64")
    else:
        frame["fill_ts_ms"] = numeric(frame["fill_ts_ms"]).astype("Int64")

    if "fill_ts_utc" not in frame.columns:
        if "ts_event" in frame.columns:
            frame["fill_ts_utc"] = ns_to_utc(frame["ts_event"])
        elif "fill_ts_ms" in frame.columns:
            frame["fill_ts_utc"] = ms_to_utc(frame["fill_ts_ms"])

    if "instrument_id" in frame.columns:
        parsed = pd.DataFrame([parse_instrument_id(value) for value in frame["instrument_id"]])
        for column in ("symbol", "venue", "product"):
            if column not in frame.columns:
                frame[column] = parsed[column]

    if "order_side" in frame.columns:
        frame["order_side"] = frame["order_side"].astype(str).str.upper()

    frame["fill_key"] = _build_fill_key(frame)
    return frame


def _coalesce_numeric_columns(frame: pd.DataFrame, candidates: tuple[str, ...]) -> pd.Series:
    resolved: pd.Series | None = None
    for column in candidates:
        if column not in frame.columns:
            continue
        values = numeric(frame[column])
        resolved = values if resolved is None else resolved.fillna(values)
    if resolved is not None:
        return resolved
    return pd.Series([math.nan] * len(frame), index=frame.index, dtype=float)


def _resolve_fill_quantity_series(frame: pd.DataFrame, *, qty_col: str = "last_qty") -> pd.Series:
    if qty_col == "last_qty":
        candidates = ("last_qty_base", "fill_qty_base", "last_qty", "fill_qty")
    elif qty_col == "fill_qty":
        candidates = ("fill_qty_base", "last_qty_base", "fill_qty", "last_qty")
    else:
        candidates = (qty_col,)
    return _coalesce_numeric_columns(frame, candidates)


def _resolve_fill_base_quantity_series(frame: pd.DataFrame) -> pd.Series:
    return _coalesce_numeric_columns(frame, ("last_qty_base", "fill_qty_base"))


def _resolve_fill_venue_quantity_series(frame: pd.DataFrame) -> pd.Series:
    return _coalesce_numeric_columns(frame, ("last_qty_venue", "fill_qty_venue", "last_qty", "fill_qty"))


def enrich_markouts(markouts: pd.DataFrame) -> pd.DataFrame:
    frame = markouts.copy()
    if "fill_qty_num" not in frame.columns and "fill_qty" in frame.columns:
        frame["fill_qty_num"] = numeric(frame["fill_qty"])
    if "fill_px_num" not in frame.columns and "fill_px" in frame.columns:
        frame["fill_px_num"] = numeric(frame["fill_px"])
    if "fill_notional" not in frame.columns:
        if "notional" in frame.columns:
            frame["fill_notional"] = numeric(frame["notional"])
        elif "fill_qty_num" in frame.columns and "fill_px_num" in frame.columns:
            frame["fill_notional"] = frame["fill_qty_num"].abs() * frame["fill_px_num"]
    else:
        frame["fill_notional"] = numeric(frame["fill_notional"])
    if "markout_bps_num" not in frame.columns:
        source = "markout_bps" if "markout_bps" in frame.columns else None
        frame["markout_bps_num"] = numeric(frame[source]) if source else pd.Series(dtype=float)
    else:
        frame["markout_bps_num"] = numeric(frame["markout_bps_num"])
    if "target_ts_utc" not in frame.columns and "target_ts_ms" in frame.columns:
        frame["target_ts_utc"] = ms_to_utc(frame["target_ts_ms"])
    if "benchmark_ts_utc" not in frame.columns and "benchmark_ts_ms" in frame.columns:
        frame["benchmark_ts_utc"] = ms_to_utc(frame["benchmark_ts_ms"])
    if "instrument_id" in frame.columns and not {"symbol", "venue", "product"}.issubset(frame.columns):
        parsed = pd.DataFrame([parse_instrument_id(value) for value in frame["instrument_id"]])
        for column in ("symbol", "venue", "product"):
            if column not in frame.columns:
                frame[column] = parsed[column]
    if "order_side" in frame.columns:
        frame["order_side"] = frame["order_side"].astype(str).str.upper()
    frame["fill_key"] = _build_fill_key(frame)
    return frame


def extract_quote_cycle_deque_diagnostics(quote_cycles: pd.DataFrame) -> pd.DataFrame:
    rows: list[AuditRow] = []
    for record in quote_cycles.itertuples(index=False):
        decision_context = _json_mapping(getattr(record, "decision_context_json", None))
        bounded = decision_context.get("bounded_convergence")
        if not isinstance(bounded, dict):
            continue
        for side, side_payload in bounded.items():
            if not isinstance(side_payload, dict):
                continue
            rows.append(
                {
                    "quote_cycle_id": getattr(record, "quote_cycle_id", None),
                    "quote_cycle_seq": getattr(record, "quote_cycle_seq", None),
                    "quote_cycle_event": getattr(record, "quote_cycle_event", None),
                    "reason_code": getattr(record, "reason_code", None),
                    "side": str(side),
                    "stack_action_mode": side_payload.get("stack_action_mode"),
                    "front_changed": side_payload.get("front_changed"),
                    "back_changed": side_payload.get("back_changed"),
                    "depth_before": side_payload.get("depth_before"),
                    "depth_after": side_payload.get("depth_after"),
                    "missing_level_count": side_payload.get("missing_level_count"),
                    "interior_hole_count": side_payload.get("interior_hole_count"),
                    "planned_cancel_count": side_payload.get("planned_cancel_count"),
                    "executed_cancel_count": side_payload.get("executed_cancel_count"),
                    "planned_place_count": side_payload.get("planned_place_count"),
                    "executed_place_count": side_payload.get("executed_place_count"),
                    "ts_cycle_end_ns": getattr(record, "ts_cycle_end_ns", None),
                },
            )
    return pd.DataFrame.from_records(rows)


def extract_order_action_deque_audit(order_actions: pd.DataFrame) -> pd.DataFrame:
    columns = [
        column
        for column in (
            "quote_cycle_id",
            "reason_code",
            "level_index",
            "client_order_id",
            "order_status",
            "side",
            "ts_decision_ns",
            "ts_submit_local_ns",
            "ts_cancel_request_local_ns",
        )
        if column in order_actions.columns
    ]
    frame = order_actions[columns].copy()
    if "reason_code" in frame.columns:
        frame = frame[frame["reason_code"].notna()]
    return frame.convert_dtypes()


def merge_fills_and_markouts(fills: pd.DataFrame, markouts: pd.DataFrame) -> pd.DataFrame:
    fill_frame = enrich_fills(fills)
    markout_frame = enrich_markouts(markouts)
    fill_context = fill_frame[
        [
            column
            for column in (
                "fill_key",
                "trader_id",
                "event_id",
                "trade_id",
                "strategy_id",
                "instrument_id",
                "symbol",
                "venue",
                "product",
                "order_side",
                "fill_px_num",
                "fill_qty_num",
                "fill_qty_base_num",
                "fill_qty_venue_num",
                "fill_notional",
                "fill_ts_ms",
                "fill_ts_utc",
                "client_order_id",
                "quote_cycle_id",
                "reason_code",
                "level_index",
                "run_id",
                "commission",
            )
            if column in fill_frame.columns
        ]
    ].drop_duplicates("fill_key")

    merged = markout_frame.merge(fill_context, on="fill_key", how="left", suffixes=("", "_fill"))
    for column in ("trader_id", "event_id", "trade_id", "strategy_id", "instrument_id", "order_side"):
        fill_column = f"{column}_fill"
        if fill_column in merged.columns:
            merged[column] = merged[column].fillna(merged[fill_column])
            merged = merged.drop(columns=[fill_column])
    for column in ("symbol", "venue", "product"):
        fill_column = f"{column}_fill"
        if fill_column in merged.columns:
            merged[column] = merged[column].combine_first(merged[fill_column])
            merged = merged.drop(columns=[fill_column])
    for column in (
        "fill_px_num",
        "fill_qty_num",
        "fill_qty_base_num",
        "fill_qty_venue_num",
        "fill_notional",
        "fill_ts_ms",
        "fill_ts_utc",
    ):
        fill_column = f"{column}_fill"
        if fill_column in merged.columns:
            merged[column] = merged[fill_column].combine_first(merged[column])
            merged = merged.drop(columns=[fill_column])
    return merged


def summarize_markouts(
    data: pd.DataFrame | None = None,
    *,
    fills: pd.DataFrame | None = None,
    markouts: pd.DataFrame | None = None,
    horizons: tuple[int, ...] = (30, 60, 120),
) -> pd.DataFrame:
    frame = _resolve_summary_frame(data=data, fills=fills, markouts=markouts)
    return pd.DataFrame([_summarize_markout_slice(frame, horizons)])


def summarize_markouts_by_side(
    data: pd.DataFrame | None = None,
    *,
    fills: pd.DataFrame | None = None,
    markouts: pd.DataFrame | None = None,
    horizons: tuple[int, ...] = (30, 60, 120),
) -> pd.DataFrame:
    return summarize_markouts_by_group(
        data,
        fills=fills,
        markouts=markouts,
        group_cols=("order_side",),
        horizons=horizons,
    )


def summarize_markouts_by_group(
    data: pd.DataFrame | None = None,
    *,
    fills: pd.DataFrame | None = None,
    markouts: pd.DataFrame | None = None,
    group_cols: str | list[str] | tuple[str, ...],
    horizons: tuple[int, ...] = (30, 60, 120),
) -> pd.DataFrame:
    frame = _resolve_summary_frame(data=data, fills=fills, markouts=markouts)
    group_list = _normalize_group_cols(group_cols)
    if not group_list:
        return summarize_markouts(frame, horizons=horizons)
    rows: list[dict[str, Any]] = []
    for group_key, group_frame in frame.groupby(group_list, dropna=False, sort=True):
        if not isinstance(group_key, tuple):
            group_key = (group_key,)
        row = {column: value for column, value in zip(group_list, group_key)}
        row.update(_summarize_markout_slice(group_frame, horizons))
        rows.append(row)
    return pd.DataFrame(rows)


def load_fv_rows(path: Path | str) -> pd.DataFrame:
    extract_path = Path(path)
    if not extract_path.exists():
        raise FileNotFoundError(extract_path)
    if extract_path.suffix == ".parquet":
        frame = pd.read_parquet(extract_path)
    else:
        frame = pd.read_csv(extract_path)
    for column in ("ts_ms", "fv", "maker_mid", "reference_mid"):
        if column in frame.columns:
            frame[column] = numeric(frame[column])
    return frame.sort_values(["strategy_id", "ts_ms"]).reset_index(drop=True)


def lookup_benchmark_at_ts(
    *,
    rows: pd.DataFrame,
    fv_rows: pd.DataFrame,
    benchmark_name: str,
    timestamp_col: str,
    direction: str,
) -> pd.DataFrame:
    index = _build_benchmark_index(fv_rows, benchmark_name)
    output_rows: list[dict[str, Any]] = []
    for row in rows.to_dict("records"):
        lookup_mode = {
            "backward": "at_or_before",
            "forward": "at_or_after",
            "nearest": "nearest",
        }[direction]
        lookup = _lookup_index_row(
            index.get(str(row.get("strategy_id"))),
            _coerce_int(row.get(timestamp_col)),
            mode=lookup_mode,
        )
        output = dict(row)
        output["benchmark_px"] = lookup["benchmark_px"]
        output["benchmark_ts_ms"] = lookup["benchmark_ts_ms"]
        output["status"] = lookup["status"]
        if lookup["status"] != "ok":
            output["lag_ms"] = math.nan
        elif direction == "backward":
            output["lag_ms"] = int(row[timestamp_col]) - int(lookup["benchmark_ts_ms"])
        elif direction == "forward":
            output["lag_ms"] = int(lookup["benchmark_ts_ms"]) - int(row[timestamp_col])
        else:
            output["lag_ms"] = abs(int(lookup["benchmark_ts_ms"]) - int(row[timestamp_col]))
        output_rows.append(output)
    return pd.DataFrame(output_rows)


def compute_fill_time_edge_rows(
    fills: pd.DataFrame,
    fv_rows: pd.DataFrame,
    benchmark_names: tuple[str, ...] = ("fv", "maker_mid"),
) -> pd.DataFrame:
    fill_frame = enrich_fills(fills)
    output_rows: list[dict[str, Any]] = []
    for benchmark_name in benchmark_names:
        looked_up = lookup_benchmark_at_ts(
            rows=fill_frame[["trader_id", "event_id", "strategy_id", "order_side", "fill_px_num", "fill_ts_ms"]],
            fv_rows=fv_rows,
            benchmark_name=benchmark_name,
            timestamp_col="fill_ts_ms",
            direction="nearest",
        )
        for row in looked_up.to_dict("records"):
            edge_abs = math.nan
            edge_bps = math.nan
            if row["status"] == "ok":
                edge_abs = _signed_markout_float(row["order_side"], row["fill_px_num"], row["benchmark_px"])
                edge_bps = _bps_float(edge_abs, row["fill_px_num"])
            output_rows.append(
                {
                    "trader_id": row["trader_id"],
                    "event_id": row["event_id"],
                    "strategy_id": row["strategy_id"],
                    "fill_ts_ms": row["fill_ts_ms"],
                    "benchmark_ts_ms": row["benchmark_ts_ms"],
                    "lag_ms": row["lag_ms"],
                    "benchmark_px": row["benchmark_px"],
                    "status": row["status"],
                    "benchmark_name": benchmark_name,
                    "edge_abs": edge_abs,
                    "edge_bps": edge_bps,
                },
            )
    return pd.DataFrame(output_rows)


def compute_extended_markouts_from_fv_stream(
    fills: pd.DataFrame,
    fv_rows: pd.DataFrame,
    *,
    horizons_s: tuple[int, ...] = (60, 120, 300, 1800, 3600),
    benchmark_names: tuple[str, ...] = ("fv", "maker_mid"),
) -> pd.DataFrame:
    fill_frame = enrich_fills(fills)
    output_rows: list[dict[str, Any]] = []
    rows: list[dict[str, Any]] = []
    for horizon_s in horizons_s:
        horizon_rows = fill_frame[["trader_id", "event_id", "strategy_id", "order_side", "fill_px_num", "fill_ts_ms"]].copy()
        horizon_rows["target_ts_ms"] = horizon_rows["fill_ts_ms"] + (int(horizon_s) * 1000)
        for benchmark_name in benchmark_names:
            looked_up = lookup_benchmark_at_ts(
                rows=horizon_rows,
                fv_rows=fv_rows,
                benchmark_name=benchmark_name,
                timestamp_col="target_ts_ms",
                direction="forward",
            )
            for row in looked_up.to_dict("records"):
                markout_abs = math.nan
                markout_bps = math.nan
                if row["status"] == "ok":
                    markout_abs = _signed_markout_float(row["order_side"], row["fill_px_num"], row["benchmark_px"])
                    markout_bps = _bps_float(markout_abs, row["fill_px_num"])
                output_rows.append(
                    {
                        "trader_id": row["trader_id"],
                        "event_id": row["event_id"],
                        "strategy_id": row["strategy_id"],
                        "target_ts_ms": row["target_ts_ms"],
                        "benchmark_ts_ms": row["benchmark_ts_ms"],
                        "benchmark_px": row["benchmark_px"],
                        "lag_ms": row["lag_ms"],
                        "status": row["status"],
                        "benchmark_name": benchmark_name,
                        "horizon_s": int(horizon_s),
                        "markout_abs": markout_abs,
                        "markout_bps": markout_bps,
                    },
                )
    return pd.DataFrame(output_rows)


def latest_order_action_context(order_actions: pd.DataFrame) -> pd.DataFrame:
    if order_actions.empty:
        return order_actions.copy()
    frame = order_actions.copy()
    frame["ts_event_num"] = numeric(frame["ts_event"])
    rich_columns = [
        column
        for column in ("quote_cycle_id", "reason_code", "level_index", "decision_context_json")
        if column in frame.columns
    ]
    if rich_columns:
        frame["context_score"] = frame[rich_columns].notna().sum(axis=1)
        frame = frame.sort_values(["context_score", "ts_event_num"], ascending=[False, False])
    else:
        frame = frame.sort_values("ts_event_num", ascending=False)
    key_columns = [column for column in ("trader_id", "client_order_id") if column in frame.columns]
    if not key_columns:
        key_columns = ["client_order_id"]
    return frame.drop_duplicates(key_columns).drop(columns=[column for column in ("ts_event_num", "context_score") if column in frame.columns])


def latest_balance_position_rows(balance_rows: pd.DataFrame) -> pd.DataFrame:
    if balance_rows.empty:
        return balance_rows.copy()
    frame = balance_rows.copy()
    frame["ts_ms_num"] = numeric(frame["ts_ms"])
    latest_snapshot = (
        frame.loc[frame["kind"] == "position", ["strategy_id", "snapshot_id", "ts_ms_num"]]
        .sort_values(["strategy_id", "ts_ms_num"], ascending=[True, False])
        .drop_duplicates(["strategy_id"])
    )
    if latest_snapshot.empty:
        return frame.iloc[0:0].copy()
    merged = frame.merge(
        latest_snapshot[["strategy_id", "snapshot_id"]],
        on=["strategy_id", "snapshot_id"],
        how="inner",
    )
    merged = merged.loc[merged["kind"] == "position"].copy()
    for column in ("signed_qty", "quantity", "avg_px_open", "realized_pnl"):
        if column in merged.columns:
            merged[f"{column}_num"] = numeric(merged[column])
    merged["ts_utc"] = ms_to_utc(merged["ts_ms"])
    return merged.drop(columns=["ts_ms_num"], errors="ignore")


def latest_portfolio_inventory(portfolio_rows: pd.DataFrame) -> pd.DataFrame:
    if portfolio_rows.empty:
        return portfolio_rows.copy()
    frame = portfolio_rows.copy()
    frame["ts_ms_num"] = numeric(frame["ts_ms"])
    latest = frame.sort_values("ts_ms_num", ascending=False).head(1).copy()
    return latest.drop(columns=["ts_ms_num"], errors="ignore")


def _build_fill_key(frame: pd.DataFrame) -> pd.Series:
    if all(column in frame.columns for column in FILL_ID_COLUMNS):
        return frame["trader_id"].astype(str) + "|" + frame["event_id"].astype(str)
    if "trade_id" in frame.columns:
        return frame["trade_id"].astype(str)
    return frame.index.astype(str)


def _normalize_group_cols(
    group_cols: str | list[str] | tuple[str, ...] | None,
) -> list[str]:
    if group_cols is None:
        return []
    if isinstance(group_cols, str):
        return [group_cols]
    return list(group_cols)


def _prepare_markout_summary_frame(data: pd.DataFrame) -> pd.DataFrame:
    frame = enrich_markouts(data)
    if "fill_notional" not in frame.columns:
        raise KeyError("markout summary frame needs a notional/fill_notional column")
    if "horizon_s" not in frame.columns:
        raise KeyError("markout summary frame needs horizon_s")
    if "resolution_status" not in frame.columns:
        frame["resolution_status"] = "resolved"
    return frame


def _resolve_summary_frame(
    *,
    data: pd.DataFrame | None,
    fills: pd.DataFrame | None,
    markouts: pd.DataFrame | None,
) -> pd.DataFrame:
    if fills is not None and markouts is not None:
        return _prepare_markout_summary_frame(merge_fills_and_markouts(fills, markouts))
    if data is not None:
        return _prepare_markout_summary_frame(data)
    raise TypeError("provide either data or both fills and markouts")


def _summarize_markout_slice(frame: pd.DataFrame, horizons: tuple[int, ...]) -> dict[str, Any]:
    unique_fills = frame.drop_duplicates("fill_key")
    summary: dict[str, Any] = {
        "fill_count": int(unique_fills["fill_key"].nunique()),
        "gross_notional": float(numeric(unique_fills["fill_notional"]).fillna(0.0).sum()),
    }
    horizon_values = numeric(frame["horizon_s"])
    for horizon_s in horizons:
        label = f"{int(horizon_s)}s"
        resolved = frame.loc[
            (horizon_values == int(horizon_s))
            & (frame["resolution_status"] == "resolved")
            & frame["markout_bps_num"].notna()
        ].copy()
        summary[f"resolved_rows_{label}"] = int(len(resolved))
        summary[f"avg_markout_bps_{label}"] = float(resolved["markout_bps_num"].mean()) if not resolved.empty else math.nan
        weights = numeric(resolved["fill_notional"])
        if resolved.empty or not (weights > 0).any():
            summary[f"nw_markout_bps_{label}"] = math.nan
        else:
            mask = weights > 0
            summary[f"nw_markout_bps_{label}"] = float(
                (resolved.loc[mask, "markout_bps_num"] * weights.loc[mask]).sum() / weights.loc[mask].sum(),
            )
    return summary


def _build_benchmark_index(
    fv_rows: pd.DataFrame,
    field: str,
) -> dict[str, dict[str, list[float | int]]]:
    if field not in fv_rows.columns:
        return {}
    frame = fv_rows.loc[fv_rows[field].notna(), ["strategy_id", "ts_ms", field]].copy()
    frame["ts_ms"] = numeric(frame["ts_ms"])
    frame[field] = numeric(frame[field])
    frame = frame.dropna(subset=["strategy_id", "ts_ms", field]).sort_values(["strategy_id", "ts_ms"])
    index: dict[str, dict[str, list[float | int]]] = {}
    for strategy_id, group in frame.groupby("strategy_id", sort=True):
        index[str(strategy_id)] = {
            "ts": [int(value) for value in group["ts_ms"].tolist()],
            "px": [float(value) for value in group[field].tolist()],
        }
    return index


def _lookup_index_row(
    strategy_rows: dict[str, list[float | int]] | None,
    target_ts_ms: int | None,
    *,
    mode: str,
) -> dict[str, Any]:
    if strategy_rows is None or target_ts_ms is None:
        return {
            "benchmark_px": None,
            "benchmark_ts_ms": None,
            "lag_ms": None,
            "status": "missing_benchmark",
        }

    timestamps = strategy_rows["ts"]
    prices = strategy_rows["px"]
    if not timestamps:
        return {
            "benchmark_px": None,
            "benchmark_ts_ms": None,
            "lag_ms": None,
            "status": "missing_benchmark",
        }

    position = bisect_left(timestamps, target_ts_ms)
    if mode == "at_or_after":
        if position >= len(timestamps):
            return {
                "benchmark_px": None,
                "benchmark_ts_ms": None,
                "status": "missing_benchmark",
            }
        benchmark_ts_ms = int(timestamps[position])
        return {
            "benchmark_px": float(prices[position]),
            "benchmark_ts_ms": benchmark_ts_ms,
            "status": "ok",
        }
    if mode == "at_or_before":
        if position == 0 and int(timestamps[0]) > int(target_ts_ms):
            return {
                "benchmark_px": None,
                "benchmark_ts_ms": None,
                "status": "missing_benchmark",
            }
        chosen = position if position < len(timestamps) and int(timestamps[position]) == int(target_ts_ms) else position - 1
        if chosen < 0:
            return {
                "benchmark_px": None,
                "benchmark_ts_ms": None,
                "status": "missing_benchmark",
            }
        benchmark_ts_ms = int(timestamps[chosen])
        return {
            "benchmark_px": float(prices[chosen]),
            "benchmark_ts_ms": benchmark_ts_ms,
            "status": "ok",
        }
    if mode == "nearest":
        candidates: list[tuple[int, int, float]] = []
        if position < len(timestamps):
            ts_value = int(timestamps[position])
            candidates.append((abs(ts_value - int(target_ts_ms)), ts_value, float(prices[position])))
        if position > 0:
            ts_value = int(timestamps[position - 1])
            candidates.append((abs(ts_value - int(target_ts_ms)), ts_value, float(prices[position - 1])))
        if not candidates:
            return {
                "benchmark_px": None,
                "benchmark_ts_ms": None,
                "lag_ms": None,
                "status": "missing_benchmark",
            }
        _, benchmark_ts_ms, benchmark_px = min(candidates, key=lambda item: (item[0], item[1]))
        return {
            "benchmark_px": benchmark_px,
            "benchmark_ts_ms": benchmark_ts_ms,
            "status": "ok",
        }
    raise ValueError(f"unsupported lookup mode {mode!r}")


def _signed_markout_float(side: Any, fill_px: Any, benchmark_px: Any) -> float:
    value = signed_markout(
        str(side),
        Decimal(str(fill_px)),
        Decimal(str(benchmark_px)),
    )
    return float(value)


def _bps_float(markout_abs: Any, fill_px: Any) -> float:
    fill_px_value = float(fill_px)
    if fill_px_value <= 0:
        return math.nan
    return float(markout_abs) / fill_px_value * 10_000.0


def _coerce_int(value: Any) -> int | None:
    if value is None or pd.isna(value):
        return None
    return int(value)
