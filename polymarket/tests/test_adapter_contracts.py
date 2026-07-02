from __future__ import annotations

import json
from decimal import Decimal
from pathlib import Path

import pandas as pd
import pytest

from polymarket.adapters.live_ws_v1 import LiveWsV1Adapter
from polymarket.adapters.pmxt_parquet_v1 import PMXTParquetV1Adapter


def test_pmxt_unknown_event_type_fails_loudly(tmp_path: Path) -> None:
    parquet_path = tmp_path / "unknown_event.parquet"
    pd.DataFrame(
        [
            {
                "timestamp_received": pd.Timestamp("2026-01-01T00:00:00Z"),
                "timestamp": pd.Timestamp("2026-01-01T00:00:00Z"),
                "market": "m1",
                "event_type": "unexpected_event",
                "asset_id": "yes",
            },
        ],
    ).to_parquet(parquet_path, index=False)

    with pytest.raises(ValueError, match="unsupported PMXT event_type"):
        PMXTParquetV1Adapter(repo_root=Path.cwd()).load({"input": {"parquet_path": str(parquet_path)}})


def test_pmxt_unknown_prebook_event_type_is_not_hidden_by_drop_until_first_book(tmp_path: Path) -> None:
    parquet_path = tmp_path / "unknown_prebook_then_book.parquet"
    pd.DataFrame(
        [
            {
                "timestamp_received": pd.Timestamp("2026-01-01T00:00:00Z"),
                "timestamp": pd.Timestamp("2026-01-01T00:00:00Z"),
                "market": "m1",
                "event_type": "unexpected_event",
                "asset_id": "yes",
            },
            {
                "timestamp_received": pd.Timestamp("2026-01-01T00:00:01Z"),
                "timestamp": pd.Timestamp("2026-01-01T00:00:01Z"),
                "market": "m1",
                "event_type": "book",
                "asset_id": "yes",
                "bids": '[["0.40", "10"]]',
                "asks": '[["0.60", "10"]]',
            },
        ],
    ).to_parquet(parquet_path, index=False)

    with pytest.raises(ValueError, match="unsupported PMXT event_type"):
        PMXTParquetV1Adapter(repo_root=Path.cwd()).load(
            {"input": {"parquet_path": str(parquet_path), "drop_until_first_book": True}},
        )


def test_pmxt_replay_order_is_explicit(tmp_path: Path) -> None:
    parquet_path = tmp_path / "ordering.parquet"
    pd.DataFrame(
        [
            {
                "timestamp_received": pd.Timestamp("2026-01-01T00:00:00Z"),
                "timestamp": pd.Timestamp("2026-01-01T00:00:02Z"),
                "market": "m1",
                "event_type": "book",
                "asset_id": "yes",
                "bids": '[["0.50", "10"]]',
                "asks": '[["0.60", "10"]]',
            },
            {
                "timestamp_received": pd.Timestamp("2026-01-01T00:00:01Z"),
                "timestamp": pd.Timestamp("2026-01-01T00:00:01Z"),
                "market": "m1",
                "event_type": "book",
                "asset_id": "yes",
                "bids": '[["0.55", "10"]]',
                "asks": '[["0.65", "10"]]',
            },
        ],
    ).to_parquet(parquet_path, index=False)
    adapter = PMXTParquetV1Adapter(repo_root=Path.cwd())

    received = adapter.load({"input": {"parquet_path": str(parquet_path), "replay_order": "received_time"}})
    source = adapter.load({"input": {"parquet_path": str(parquet_path), "replay_order": "source_time"}})

    assert received.steps[0].updates[0].best_bid is None
    assert received.steps[0].updates[0].bids[0].price == Decimal("0.50")
    assert source.steps[0].updates[0].bids[0].price == Decimal("0.55")


def test_live_ws_unknown_event_type_fails_loudly(tmp_path: Path) -> None:
    ndjson_path = tmp_path / "unknown_live.ndjson"
    ndjson_path.write_text(
        json.dumps(
            {
                "local_msg_index": 1,
                "recv_wall_time_utc": "2026-01-01T00:00:00Z",
                "raw_json": {"event_type": "unexpected_event", "timestamp": "2026-01-01T00:00:00Z"},
            },
        )
        + "\n",
        encoding="utf-8",
    )

    with pytest.raises(ValueError, match="unsupported live WS event_type"):
        LiveWsV1Adapter(repo_root=Path.cwd()).load({"input": {"ndjson_path": str(ndjson_path)}})


def test_live_ws_control_messages_are_explicitly_skipped_with_warning(tmp_path: Path) -> None:
    ndjson_path = tmp_path / "control_then_book.ndjson"
    lines = [
        {
            "local_msg_index": 1,
            "recv_wall_time_utc": "2026-01-01T00:00:00Z",
            "raw_json": {"type": "PING", "timestamp": "2026-01-01T00:00:00Z"},
        },
        {
            "local_msg_index": 2,
            "recv_wall_time_utc": "2026-01-01T00:00:01Z",
            "raw_json": {
                "event_type": "book",
                "market": "m1",
                "asset_id": "yes",
                "timestamp": "2026-01-01T00:00:01Z",
                "bids": [["0.40", "10"]],
                "asks": [["0.60", "10"]],
            },
        },
    ]
    ndjson_path.write_text("".join(json.dumps(line) + "\n" for line in lines), encoding="utf-8")

    dataset = LiveWsV1Adapter(repo_root=Path.cwd()).load({"input": {"ndjson_path": str(ndjson_path)}})

    assert len(dataset.steps) == 1
    assert dataset.steps[0].sequence == 2
    assert any("Skipped 1 explicit live WS control" in warning for warning in dataset.metadata.warnings)
