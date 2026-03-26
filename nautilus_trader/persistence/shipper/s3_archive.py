from __future__ import annotations

from dataclasses import dataclass
from datetime import UTC
from datetime import datetime
from pathlib import Path
import re
import sqlite3
from typing import Any

import pyarrow as pa
import pyarrow.parquet as pq

from nautilus_trader.flux.persistence.balance_snapshots.schema import (
    FLUX_BALANCE_SNAPSHOT_COLUMN_NAMES,
)
from nautilus_trader.flux.persistence.balance_snapshots.schema import (
    FLUX_BALANCE_SNAPSHOT_ROW_COLUMN_NAMES,
)
from nautilus_trader.flux.persistence.portfolio_inventory_snapshots.schema import (
    PORTFOLIO_INVENTORY_SNAPSHOT_COLUMN_NAMES,
)
from nautilus_trader.persistence.fills.schema import EXECUTION_FILL_COLUMN_NAMES
from nautilus_trader.persistence.orders.schema import ORDER_ACTION_COLUMN_NAMES


@dataclass(frozen=True, slots=True)
class TelemetryArchiveSpec:
    dataset_name: str
    source_table_name: str
    columns: tuple[str, ...]
    created_at_column: str = "created_at"
    strategy_id_column: str | None = "strategy_id"


@dataclass(frozen=True, slots=True)
class TelemetryArchiveResult:
    dataset_name: str
    row_count: int
    parquet_path: Path
    s3_key: str
    athena_table: str
    athena_ddl: str
    athena_partition_sql: str
    event_date: str
    strategy_partition: str
    local_db_deleted: bool = False


TELEMETRY_DB_ARCHIVE_SPECS: dict[str, tuple[TelemetryArchiveSpec, ...]] = {
    "orders.sqlite": (
        TelemetryArchiveSpec(
            dataset_name="order_action",
            source_table_name="order_action",
            columns=ORDER_ACTION_COLUMN_NAMES,
        ),
    ),
    "fills.sqlite": (
        TelemetryArchiveSpec(
            dataset_name="execution_fill",
            source_table_name="execution_fill",
            columns=EXECUTION_FILL_COLUMN_NAMES,
        ),
    ),
    "balance_snapshots.sqlite": (
        TelemetryArchiveSpec(
            dataset_name="flux_balance_snapshot",
            source_table_name="flux_balance_snapshot",
            columns=FLUX_BALANCE_SNAPSHOT_COLUMN_NAMES,
        ),
        TelemetryArchiveSpec(
            dataset_name="flux_balance_snapshot_row",
            source_table_name="flux_balance_snapshot_row",
            columns=FLUX_BALANCE_SNAPSHOT_ROW_COLUMN_NAMES,
        ),
    ),
    "portfolio_inventory.sqlite": (
        TelemetryArchiveSpec(
            dataset_name="portfolio_inventory_snapshot",
            source_table_name="portfolio_inventory_snapshot",
            columns=PORTFOLIO_INVENTORY_SNAPSHOT_COLUMN_NAMES,
            strategy_id_column=None,
        ),
    ),
}


def archive_rotated_sqlite_database(
    *,
    db_path: Path,
    staging_root: Path,
    source_profile: str,
    bucket: str,
    prefix: str,
    athena_database: str = "nautilus_telemetry",
) -> tuple[TelemetryArchiveResult, ...]:
    specs = TELEMETRY_DB_ARCHIVE_SPECS.get(db_path.name, ())
    results: list[TelemetryArchiveResult] = []
    for spec in specs:
        result = archive_sqlite_table(
            db_path=db_path,
            spec=spec,
            staging_root=staging_root,
            source_profile=source_profile,
            bucket=bucket,
            prefix=prefix,
            athena_database=athena_database,
        )
        if result is not None:
            results.append(result)
    return tuple(results)


def archive_sqlite_table(
    *,
    db_path: Path,
    spec: TelemetryArchiveSpec,
    staging_root: Path,
    source_profile: str,
    bucket: str,
    prefix: str,
    athena_database: str = "nautilus_telemetry",
    delete_local_after_archive: bool = False,
) -> TelemetryArchiveResult | None:
    rows = _load_rows(db_path=db_path, spec=spec)
    if not rows:
        return None

    event_date = _event_date_from_rows(rows, spec.created_at_column)
    strategy_partition = _strategy_partition_from_rows(rows, spec.strategy_id_column)
    s3_key = build_s3_key(
        prefix=prefix,
        source_profile=source_profile,
        dataset_name=spec.dataset_name,
        event_date=event_date,
        strategy_partition=strategy_partition,
        file_stem=f"{db_path.stem}-{spec.dataset_name}",
    )
    parquet_path = staging_root / s3_key
    parquet_path.parent.mkdir(parents=True, exist_ok=True)

    table = pa.Table.from_pylist(rows)
    pq.write_table(table, parquet_path)
    athena_table = build_athena_table_name(source_profile=source_profile, dataset_name=spec.dataset_name)
    athena_ddl = build_athena_external_table_sql(
        database=athena_database,
        table_name=athena_table,
        bucket=bucket,
        prefix=prefix,
        source_profile=source_profile,
        dataset_name=spec.dataset_name,
        schema=table.schema,
    )
    athena_partition_sql = build_athena_add_partition_sql(
        database=athena_database,
        table_name=athena_table,
        bucket=bucket,
        prefix=prefix,
        source_profile=source_profile,
        dataset_name=spec.dataset_name,
        event_date=event_date,
        strategy_partition=strategy_partition,
    )

    if delete_local_after_archive:
        db_path.unlink(missing_ok=True)

    return TelemetryArchiveResult(
        dataset_name=spec.dataset_name,
        row_count=len(rows),
        parquet_path=parquet_path,
        s3_key=s3_key,
        athena_table=athena_table,
        athena_ddl=athena_ddl,
        athena_partition_sql=athena_partition_sql,
        event_date=event_date,
        strategy_partition=strategy_partition,
        local_db_deleted=delete_local_after_archive,
    )


def build_s3_key(
    *,
    prefix: str,
    source_profile: str,
    dataset_name: str,
    event_date: str,
    strategy_partition: str,
    file_stem: str,
) -> str:
    return (
        f"{prefix.rstrip('/')}/"
        f"source_profile={_sanitize_partition_value(source_profile)}/"
        f"dataset={_sanitize_partition_value(dataset_name)}/"
        f"event_date={event_date}/"
        f"strategy_partition={_sanitize_partition_value(strategy_partition)}/"
        f"{_sanitize_partition_value(file_stem)}.parquet"
    )


def build_athena_table_name(*, source_profile: str, dataset_name: str) -> str:
    return _sanitize_identifier(f"{source_profile}_{dataset_name}")


def build_athena_external_table_sql(
    *,
    database: str,
    table_name: str,
    bucket: str,
    prefix: str,
    source_profile: str,
    dataset_name: str,
    schema: pa.Schema,
) -> str:
    column_lines = ",\n".join(
        f"  `{field.name}` {_athena_type_for_arrow(field.type)}" for field in schema
    )
    location = (
        f"s3://{bucket}/{prefix.rstrip('/')}/"
        f"source_profile={_sanitize_partition_value(source_profile)}/"
        f"dataset={_sanitize_partition_value(dataset_name)}/"
    )
    return (
        f"CREATE EXTERNAL TABLE IF NOT EXISTS {database}.{table_name} (\n"
        f"{column_lines}\n"
        ")\n"
        "PARTITIONED BY (\n"
        "  `event_date` string,\n"
        "  `strategy_partition` string\n"
        ")\n"
        "STORED AS PARQUET\n"
        f"LOCATION '{location}';"
    )


def build_athena_add_partition_sql(
    *,
    database: str,
    table_name: str,
    bucket: str,
    prefix: str,
    source_profile: str,
    dataset_name: str,
    event_date: str,
    strategy_partition: str,
) -> str:
    location = (
        f"s3://{bucket}/{prefix.rstrip('/')}/"
        f"source_profile={_sanitize_partition_value(source_profile)}/"
        f"dataset={_sanitize_partition_value(dataset_name)}/"
        f"event_date={event_date}/"
        f"strategy_partition={_sanitize_partition_value(strategy_partition)}/"
    )
    return (
        f"ALTER TABLE {database}.{table_name} "
        "ADD IF NOT EXISTS "
        f"PARTITION (event_date='{_escape_sql_literal(event_date)}', "
        f"strategy_partition='{_escape_sql_literal(strategy_partition)}') "
        f"LOCATION '{location}';"
    )


def _load_rows(*, db_path: Path, spec: TelemetryArchiveSpec) -> list[dict[str, Any]]:
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    try:
        query = (
            f"SELECT {', '.join(spec.columns)} "
            f"FROM {spec.source_table_name} "
            "ORDER BY rowid ASC"
        )
        rows = conn.execute(query).fetchall()
    finally:
        conn.close()
    return [dict(row) for row in rows]


def _event_date_from_rows(rows: list[dict[str, Any]], column_name: str) -> str:
    for row in rows:
        value = row.get(column_name)
        if value is None:
            continue
        text = str(value).strip()
        if not text:
            continue
        if "T" in text:
            return text.split("T", 1)[0]
        if len(text) >= 10 and text[4] == "-" and text[7] == "-":
            return text[:10]
    return datetime.now(UTC).date().isoformat()


def _strategy_partition_from_rows(
    rows: list[dict[str, Any]],
    strategy_id_column: str | None,
) -> str:
    if strategy_id_column is None:
        return "all"
    values = {
        str(row[strategy_id_column]).strip()
        for row in rows
        if row.get(strategy_id_column) is not None and str(row[strategy_id_column]).strip()
    }
    if not values:
        return "all"
    if len(values) == 1:
        return next(iter(values))
    return "mixed"


def _athena_type_for_arrow(data_type: pa.DataType) -> str:
    if pa.types.is_boolean(data_type):
        return "boolean"
    if pa.types.is_integer(data_type):
        return "bigint"
    if pa.types.is_floating(data_type):
        return "double"
    return "string"


def _sanitize_identifier(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9_]+", "_", value).strip("_").lower()


def _sanitize_partition_value(value: str) -> str:
    sanitized = re.sub(r"[^A-Za-z0-9_.=-]+", "_", value).strip("_")
    return sanitized or "unknown"


def _escape_sql_literal(value: str) -> str:
    return value.replace("'", "''")
