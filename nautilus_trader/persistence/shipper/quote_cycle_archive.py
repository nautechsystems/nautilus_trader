from __future__ import annotations

from pathlib import Path

from nautilus_trader.flux.persistence.quote_cycles.schema import QUOTE_CYCLE_COLUMN_NAMES
from nautilus_trader.persistence.shipper.s3_archive import TelemetryArchiveResult
from nautilus_trader.persistence.shipper.s3_archive import TelemetryArchiveSpec
from nautilus_trader.persistence.shipper.s3_archive import archive_sqlite_table


QUOTE_CYCLE_ARCHIVE_SPEC = TelemetryArchiveSpec(
    dataset_name="quote_cycle",
    source_table_name="quote_cycle",
    columns=QUOTE_CYCLE_COLUMN_NAMES,
)


def archive_rotated_quote_cycle_db(
    *,
    db_path: Path,
    staging_root: Path,
    source_profile: str,
    bucket: str,
    prefix: str,
    athena_database: str = "nautilus_telemetry",
    delete_local_after_archive: bool = False,
) -> TelemetryArchiveResult | None:
    return archive_sqlite_table(
        db_path=db_path,
        spec=QUOTE_CYCLE_ARCHIVE_SPEC,
        staging_root=staging_root,
        source_profile=source_profile,
        bucket=bucket,
        prefix=prefix,
        athena_database=athena_database,
        delete_local_after_archive=delete_local_after_archive,
    )
