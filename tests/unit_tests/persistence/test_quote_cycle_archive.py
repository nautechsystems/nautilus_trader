from __future__ import annotations

import sqlite3
from pathlib import Path

from nautilus_trader.persistence.shipper.quote_cycle_archive import archive_rotated_quote_cycle_db


def _create_quote_cycle_db(path: Path) -> None:
    conn = sqlite3.connect(path)
    try:
        conn.execute(
            """
            CREATE TABLE quote_cycle (
              trader_id TEXT NOT NULL,
              strategy_id TEXT NOT NULL,
              instrument_id TEXT NOT NULL,
              run_id TEXT NOT NULL,
              quote_cycle_id TEXT NOT NULL,
              quote_cycle_seq INTEGER NOT NULL,
              quote_cycle_event TEXT NOT NULL,
              reason_code TEXT NOT NULL,
              trigger_source TEXT,
              trigger_instrument_id TEXT,
              trigger_md_ts_event_ns INTEGER,
              trigger_md_ts_init_ns INTEGER,
              ts_cycle_start_ns INTEGER,
              ts_cycle_end_ns INTEGER,
              state_from TEXT,
              state_to TEXT,
              cancel_count INTEGER,
              place_count INTEGER,
              bid_levels INTEGER,
              ask_levels INTEGER,
              decision_context_json TEXT NOT NULL,
              created_at TEXT NOT NULL
            )
            """,
        )
        conn.execute(
            """
            INSERT INTO quote_cycle (
              trader_id, strategy_id, instrument_id, run_id, quote_cycle_id, quote_cycle_seq,
              quote_cycle_event, reason_code, trigger_source, trigger_instrument_id,
              trigger_md_ts_event_ns, trigger_md_ts_init_ns, ts_cycle_start_ns, ts_cycle_end_ns,
              state_from, state_to, cancel_count, place_count, bid_levels, ask_levels,
              decision_context_json, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "TRADER-001",
                "strategy-a",
                "PLUMEUSDT.BINANCE_SPOT",
                "run-1",
                "run-1:1",
                1,
                "completed",
                "completed_rebalanced",
                "signal",
                "PLUMEUSDT.BINANCE_SPOT",
                1,
                2,
                3,
                4,
                "quoted",
                "completed",
                1,
                1,
                2,
                2,
                "{\"edge_bps\": 3.0}",
                "2026-03-26T17:54:00.000Z",
            ),
        )
        conn.commit()
    finally:
        conn.close()


def test_archive_rotated_quote_cycle_db_writes_parquet_and_deletes_local_segment(tmp_path: Path) -> None:
    db_path = tmp_path / "quote_cycles.sqlite.cutover-20260326T175400Z"
    _create_quote_cycle_db(db_path)

    result = archive_rotated_quote_cycle_db(
        db_path=db_path,
        staging_root=tmp_path / "staging",
        source_profile="tokenmm",
        bucket="unit-test-bucket",
        prefix="nautilus/telemetry/tokenmm",
        athena_database="ops_telemetry",
        delete_local_after_archive=True,
    )

    assert result is not None
    assert result.row_count == 1
    assert result.athena_table == "tokenmm_quote_cycle"
    assert "ALTER TABLE ops_telemetry.tokenmm_quote_cycle ADD IF NOT EXISTS PARTITION" in result.athena_partition_sql
    assert "dataset=quote_cycle" in result.s3_key
    assert "strategy_partition=strategy-a" in result.s3_key
    assert result.parquet_path.exists()
    assert result.local_db_deleted is True
    assert not db_path.exists()


def test_quote_cycle_archive_emits_deterministic_athena_ddl(tmp_path: Path) -> None:
    db_path = tmp_path / "quote_cycles.sqlite.cutover-20260326T175400Z"
    _create_quote_cycle_db(db_path)

    result = archive_rotated_quote_cycle_db(
        db_path=db_path,
        staging_root=tmp_path / "staging",
        source_profile="tokenmm",
        bucket="unit-test-bucket",
        prefix="nautilus/telemetry/tokenmm",
    )

    assert result is not None
    assert "CREATE EXTERNAL TABLE IF NOT EXISTS nautilus_telemetry.tokenmm_quote_cycle" in result.athena_ddl
    assert (
        "LOCATION 's3://unit-test-bucket/nautilus/telemetry/tokenmm/source_profile=tokenmm/dataset=quote_cycle/'"
        in result.athena_ddl
    )
