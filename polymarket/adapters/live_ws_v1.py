"""Local raw Polymarket WebSocket NDJSON adapter."""

from __future__ import annotations

import json
import os
from collections.abc import Mapping
from pathlib import Path
from typing import Any

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


class LiveWsV1Adapter:
    adapter_name = "live_ws_v1"
    adapter_version = "v1"
    control_event_types = frozenset({"ping", "pong"})

    def __init__(self, *, repo_root: Path | None = None) -> None:
        self.repo_root = repo_root or Path.cwd()

    def load(self, config: Mapping[str, Any]) -> PolymarketL2DatasetV1:
        input_config = config.get("input", config)
        raw_path = input_config.get("ndjson_path") or input_config.get("path")
        if raw_path is None and input_config.get("ndjson_path_env") is not None:
            env_name = str(input_config["ndjson_path_env"])
            raw_path = os.environ.get(env_name)
            if raw_path is None:
                raise FileNotFoundError(f"environment variable {env_name} is not set")
        ndjson_path = Path(str(raw_path))
        if not ndjson_path.is_absolute():
            ndjson_path = self.repo_root / ndjson_path
        if not ndjson_path.exists():
            raise FileNotFoundError(ndjson_path)

        steps: list[L2ReplayStepV1] = []
        skipped_control_messages = 0
        with ndjson_path.open(encoding="utf-8-sig") as f:
            for local_index, line in enumerate(f, start=1):
                stripped = line.strip()
                if not stripped:
                    continue
                payload = json.loads(stripped)
                message = self._extract_message(payload)
                updates = tuple(self._message_to_updates(message))
                if not updates:
                    skipped_control_messages += 1
                    continue
                timestamp_received = self._received_timestamp(payload, message)
                source_ts = optional_utc_datetime(message.get("timestamp"))
                steps.append(
                    L2ReplayStepV1(
                        sequence=int(payload.get("local_msg_index", local_index)),
                        timestamp_received=timestamp_received,
                        timestamp=source_ts,
                        updates=updates,
                    ),
                )

        warnings = ["No official Polymarket message id is assumed; local capture order is local evidence only."]
        if skipped_control_messages:
            warnings.append(f"Skipped {skipped_control_messages} explicit live WS control message(s).")
        metadata = DatasetMetadataV1(
            dataset_id=str(input_config.get("dataset_id") or ndjson_path.stem),
            adapter_name=self.adapter_name,
            adapter_version=self.adapter_version,
            source_type="live_raw_ws",
            source_files=(repo_relative_or_absolute(ndjson_path, repo_root=self.repo_root),),
            assumptions=("One local raw WebSocket message is one replay step.",),
            warnings=tuple(warnings),
        )
        return PolymarketL2DatasetV1(metadata=metadata, steps=tuple(steps))

    @staticmethod
    def _extract_message(payload: Mapping[str, Any]) -> Mapping[str, Any]:
        raw = payload.get("raw_json") or payload.get("message") or payload.get("data")
        if raw is None:
            return payload
        if isinstance(raw, str):
            raw = json.loads(raw)
        if isinstance(raw, list):
            if len(raw) != 1:
                raise ValueError("live_ws_v1 expects one message per NDJSON line")
            raw = raw[0]
        if not isinstance(raw, Mapping):
            raise ValueError(f"unsupported raw WebSocket payload: {type(raw)!r}")
        return raw

    @staticmethod
    def _received_timestamp(payload: Mapping[str, Any], message: Mapping[str, Any]) -> Any:
        for key in ("recv_wall_time_utc", "timestamp_received", "received_at"):
            if payload.get(key) is not None:
                return as_utc_datetime(payload[key])
        if message.get("timestamp_received") is not None:
            return as_utc_datetime(message["timestamp_received"])
        if message.get("timestamp") is not None:
            return as_utc_datetime(message["timestamp"])
        raise ValueError("live WS message lacks receive timestamp")

    def _message_to_updates(self, message: Mapping[str, Any]) -> list[L2UpdateV1]:
        event_type = str(message.get("event_type") or message.get("type") or "")
        normalized_event_type = event_type.lower()
        if normalized_event_type in self.control_event_types:
            return []
        if event_type == "price_change":
            return self._price_change_updates(message)
        if event_type == "book":
            return self._book_updates(message)
        if event_type == "last_trade_price":
            return [self._trade_update(message)]
        if event_type == "tick_size_change":
            return [self._tick_size_update(message)]
        raise ValueError(f"unsupported live WS event_type: {event_type!r}")

    @staticmethod
    def _price_change_updates(message: Mapping[str, Any]) -> list[L2UpdateV1]:
        changes = message.get("price_changes") or message.get("changes") or []
        if not changes:
            raise ValueError("live_ws_v1 price_change message has no price_changes/changes")
        updates: list[L2UpdateV1] = []
        for change in changes:
            market = str(change.get("market") or message.get("market"))
            asset_id = str(change.get("asset_id") or change.get("asset") or message.get("asset_id"))
            updates.append(
                L2UpdateV1(
                    event_type="price_change",
                    market=market,
                    asset_id=asset_id,
                    side=normalize_side(change.get("side")),  # type: ignore[arg-type]
                    price=as_decimal(change.get("price")),
                    size=as_decimal(change.get("size")),
                    best_bid=as_decimal(change.get("best_bid") or message.get("best_bid")),
                    best_ask=as_decimal(change.get("best_ask") or message.get("best_ask")),
                ),
            )
        return updates

    @staticmethod
    def _book_updates(message: Mapping[str, Any]) -> list[L2UpdateV1]:
        return [
            L2UpdateV1(
                event_type="book",
                market=str(message.get("market")),
                asset_id=str(message.get("asset_id") or message.get("asset")),
                bids=parse_levels(message.get("bids")),
                asks=parse_levels(message.get("asks")),
                best_bid=as_decimal(message.get("best_bid")),
                best_ask=as_decimal(message.get("best_ask")),
            ),
        ]

    @staticmethod
    def _trade_update(message: Mapping[str, Any]) -> L2UpdateV1:
        return L2UpdateV1(
            event_type="trade",
            market=str(message.get("market")),
            asset_id=str(message.get("asset_id") or message.get("asset")),
            price=as_decimal(message.get("price")),
            size=as_decimal(message.get("size")),
            side=normalize_side(message.get("side")),  # type: ignore[arg-type]
        )

    @staticmethod
    def _tick_size_update(message: Mapping[str, Any]) -> L2UpdateV1:
        return L2UpdateV1(
            event_type="tick_size_change",
            market=str(message.get("market")),
            asset_id=str(message.get("asset_id") or message.get("asset")),
            old_tick_size=as_decimal(message.get("old_tick_size")),
            new_tick_size=as_decimal(message.get("new_tick_size")),
        )
