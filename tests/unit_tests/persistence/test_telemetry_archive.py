from __future__ import annotations

import sqlite3
from pathlib import Path

import pytest

from nautilus_trader.persistence.shipper.config import build_telemetry_shipper_config
from nautilus_trader.persistence.shipper.s3_archive import TelemetryArchiveSpec
from nautilus_trader.persistence.shipper.s3_archive import archive_sqlite_table


def _create_archive_source(path: Path) -> None:
    conn = sqlite3.connect(path)
    try:
        conn.execute(
            """
            CREATE TABLE events (
              strategy_id TEXT NOT NULL,
              created_at TEXT NOT NULL,
              reason_code TEXT NOT NULL
            )
            """,
        )
        conn.execute(
            """
            INSERT INTO events (strategy_id, created_at, reason_code)
            VALUES (?, ?, ?)
            """,
            ("strategy-a", "2026-03-26T17:54:00.000Z", "completed"),
        )
        conn.commit()
    finally:
        conn.close()


def test_build_shipper_config_rejects_s3_athena_sink_without_bucket() -> None:
    with pytest.raises(ValueError, match="archive_s3_bucket"):
        build_telemetry_shipper_config(
            {
                "enabled": True,
                "source_profile": "tokenmm",
                "durable_sink": "s3_athena",
                "orders_db_path": "/tmp/orders.sqlite",
                "state_db_path": "/tmp/shipper_state.sqlite",
            },
            env={},
        )


def test_build_shipper_config_rejects_quote_cycle_path_when_raw_quote_cycles_disabled() -> None:
    with pytest.raises(ValueError, match="quote_cycles_db_path"):
        build_telemetry_shipper_config(
            {
                "enabled": True,
                "source_profile": "tokenmm",
                "durable_sink": "postgres",
                "raw_quote_cycles_enabled": False,
                "orders_db_path": "/tmp/orders.sqlite",
                "quote_cycles_db_path": "/tmp/quote_cycles.sqlite",
                "state_db_path": "/tmp/shipper_state.sqlite",
            },
            env={
                "NAUTILUS_TELEMETRY_PG_HOST": "localhost",
                "NAUTILUS_TELEMETRY_PG_DATABASE": "nautilus_telemetry",
                "NAUTILUS_TELEMETRY_PG_SCHEMA": "telemetry",
                "NAUTILUS_TELEMETRY_PG_USERNAME": "nautilus",
                "NAUTILUS_TELEMETRY_PG_PASSWORD": "pass",
            },
        )


def test_archive_sqlite_table_writes_parquet_and_builds_deterministic_athena_contract(
    tmp_path: Path,
) -> None:
    db_path = tmp_path / "events.sqlite"
    _create_archive_source(db_path)
    spec = TelemetryArchiveSpec(
        dataset_name="events",
        source_table_name="events",
        columns=("strategy_id", "created_at", "reason_code"),
    )

    result = archive_sqlite_table(
        db_path=db_path,
        spec=spec,
        staging_root=tmp_path / "staging",
        source_profile="tokenmm",
        bucket="unit-test-bucket",
        prefix="nautilus/telemetry",
        athena_database="ops_telemetry",
    )

    assert result is not None
    assert result.row_count == 1
    assert "source_profile=tokenmm" in result.s3_key
    assert "dataset=events" in result.s3_key
    assert "event_date=2026-03-26" in result.s3_key
    assert "strategy_partition=strategy-a" in result.s3_key
    assert result.parquet_path.exists()
    assert result.athena_table == "tokenmm_events"
    assert "CREATE EXTERNAL TABLE IF NOT EXISTS ops_telemetry.tokenmm_events" in result.athena_ddl
    assert "LOCATION 's3://unit-test-bucket/nautilus/telemetry/source_profile=tokenmm/dataset=events/'" in result.athena_ddl
    assert "ALTER TABLE ops_telemetry.tokenmm_events ADD IF NOT EXISTS PARTITION" in result.athena_partition_sql
    assert "event_date='2026-03-26'" in result.athena_partition_sql
    assert "strategy_partition='strategy-a'" in result.athena_partition_sql
