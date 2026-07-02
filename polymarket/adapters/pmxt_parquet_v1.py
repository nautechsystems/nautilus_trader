"""PMXT hourly parquet adapter for Polymarket v1.

This adapter is legacy/questionable by design.  PMXT parquet rows do not expose
raw WebSocket message boundaries, so replay steps are an adapter approximation.
"""

from __future__ import annotations

from collections.abc import Mapping
from pathlib import Path
from typing import Any

import pandas as pd

from polymarket.adapters.utils import (
    as_decimal,
    as_utc_datetime,
    normalize_side,
    optional_utc_datetime,
    parse_levels,
    repo_relative_or_absolute,
)
from polymarket.models import (
    DatasetMetadataV1,
    L2ReplayStepV1,
    L2UpdateV1,
    PolymarketL2DatasetV1,
)

PMXT_RAW_EVENT_TYPES = frozenset({"book", "price_change", "last_trade_price", "tick_size_change"})
CANONICAL_EVENT_TYPES = frozenset({"book", "price_change", "trade", "tick_size_change"})


class PMXTParquetV1Adapter:
    adapter_name = "pmxt_parquet_v1"
    adapter_version = "v1"

    def __init__(self, *, repo_root: Path | None = None) -> None:
        self.repo_root = repo_root or Path.cwd()

    def load(self, config: Mapping[str, Any]) -> PolymarketL2DatasetV1:
        input_config = config.get("input", config)
        parquet_path = Path(str(input_config.get("parquet_path") or input_config.get("path")))
        if not parquet_path.is_absolute():
            parquet_path = self.repo_root / parquet_path
        if not parquet_path.exists():
            raise FileNotFoundError(parquet_path)

        df = pd.read_parquet(parquet_path)
        df = self._filter(df, input_config)
        if df.empty:
            raise ValueError(f"PMXT parquet filter produced no rows: {parquet_path}")
        self._validate_event_types(df)

        replay_order = str(input_config.get("replay_order", "received_time"))
        df = self._sort(df, replay_order)
        if input_config.get("drop_until_first_book", False):
            df = self._drop_until_first_book(df)
        steps = tuple(self._rows_to_steps(df))
        metadata = DatasetMetadataV1(
            dataset_id=str(input_config.get("dataset_id") or parquet_path.stem),
            adapter_name=self.adapter_name,
            adapter_version=self.adapter_version,
            source_type="pmxt_parquet",
            source_files=(repo_relative_or_absolute(parquet_path, repo_root=self.repo_root),),
            assumptions=(
                "PMXT parquet lacks raw WebSocket message boundaries; replay steps are adapter approximations.",
                f"PMXT replay_order={replay_order}.",
                f"PMXT drop_until_first_book={bool(input_config.get('drop_until_first_book', False))}.",
            ),
            warnings=(
                "PMXT source timestamp may invert; timestamp_received should be preferred for no-receive-inversion samples.",
                "PMXT best_bid/best_ask is not row-level ground truth.",
            ),
        )
        return PolymarketL2DatasetV1(metadata=metadata, steps=steps)

    @staticmethod
    def _filter(df: pd.DataFrame, input_config: Mapping[str, Any]) -> pd.DataFrame:
        filtered = df.copy()
        for column, config_key in (("market", "market"), ("asset_id", "asset_id")):
            value = input_config.get(config_key)
            values = input_config.get(f"{config_key}s")
            if value is not None:
                filtered = filtered[filtered[column].astype(str) == str(value)]
            elif values is not None:
                allowed = {str(item) for item in values}
                filtered = filtered[filtered[column].astype(str).isin(allowed)]
        start = input_config.get("start") or input_config.get("start_timestamp_received")
        end = input_config.get("end") or input_config.get("end_timestamp_received")
        if start is not None:
            filtered = filtered[pd.to_datetime(filtered["timestamp_received"], utc=True) >= pd.Timestamp(start)]
        if end is not None:
            filtered = filtered[pd.to_datetime(filtered["timestamp_received"], utc=True) <= pd.Timestamp(end)]
        return filtered

    @staticmethod
    def _sort(df: pd.DataFrame, replay_order: str) -> pd.DataFrame:
        if replay_order == "source_time":
            columns = ["timestamp", "timestamp_received", "market", "asset_id", "event_type"]
        elif replay_order == "received_time":
            columns = ["timestamp_received", "timestamp", "market", "asset_id", "event_type"]
        else:
            raise ValueError(f"unsupported PMXT replay_order: {replay_order}")
        existing = [column for column in columns if column in df.columns]
        return df.sort_values(existing, kind="mergesort").reset_index(drop=True)

    @staticmethod
    def _validate_event_types(df: pd.DataFrame) -> None:
        if "event_type" not in df.columns:
            raise ValueError("PMXT parquet is missing required event_type column")
        observed = {str(value) for value in df["event_type"].dropna().unique()}
        unsupported = sorted(observed - PMXT_RAW_EVENT_TYPES)
        if unsupported:
            raise ValueError(f"unsupported PMXT event_type(s): {unsupported}")

    @staticmethod
    def _drop_until_first_book(df: pd.DataFrame) -> pd.DataFrame:
        kept: list[pd.DataFrame] = []
        for _, group in df.groupby("asset_id", sort=False):
            book_positions = group.index[group["event_type"] == "book"].tolist()
            if not book_positions:
                kept.append(group.iloc[0:0])
                continue
            first_book_index = book_positions[0]
            kept.append(group.loc[first_book_index:])
        if not kept:
            return df.iloc[0:0].copy()
        return pd.concat(kept).sort_index(kind="mergesort").reset_index(drop=True)

    def _rows_to_steps(self, df: pd.DataFrame) -> list[L2ReplayStepV1]:
        steps: list[L2ReplayStepV1] = []
        for sequence, row in enumerate(df.itertuples(index=False), start=1):
            timestamp_received = as_utc_datetime(getattr(row, "timestamp_received"))
            source_ts = optional_utc_datetime(getattr(row, "timestamp", None))
            update = self._row_to_update(row)
            steps.append(
                L2ReplayStepV1(
                    sequence=sequence,
                    timestamp_received=timestamp_received,
                    timestamp=source_ts,
                    updates=(update,),
                ),
            )
        return steps

    @staticmethod
    def _row_to_update(row: Any) -> L2UpdateV1:
        raw_event_type = str(getattr(row, "event_type"))
        event_type = "trade" if raw_event_type == "last_trade_price" else raw_event_type
        if event_type not in CANONICAL_EVENT_TYPES:
            raise ValueError(f"unsupported PMXT event_type: {raw_event_type!r}")
        market = str(getattr(row, "market"))
        asset_id = str(getattr(row, "asset_id"))
        return L2UpdateV1(
            event_type=event_type,  # type: ignore[arg-type]
            market=market,
            asset_id=asset_id,
            side=normalize_side(getattr(row, "side", None)),  # type: ignore[arg-type]
            price=as_decimal(getattr(row, "price", None)),
            size=as_decimal(getattr(row, "size", None)),
            bids=parse_levels(getattr(row, "bids", None)),
            asks=parse_levels(getattr(row, "asks", None)),
            best_bid=as_decimal(getattr(row, "best_bid", None)),
            best_ask=as_decimal(getattr(row, "best_ask", None)),
            old_tick_size=as_decimal(getattr(row, "old_tick_size", None)),
            new_tick_size=as_decimal(getattr(row, "new_tick_size", None)),
        )
