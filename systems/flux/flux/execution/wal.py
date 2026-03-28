from __future__ import annotations

import sqlite3
import sys
from dataclasses import dataclass
from pathlib import Path

from .controller import ControllerSnapshotAuthority
from .controller import SnapshotAuthorityState
from .intents import ExecutionClaim
from .intents import ExecutionLifecycleState


if __name__ == "flux.execution.wal":
    sys.modules.setdefault("nautilus_trader.flux.execution.wal", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.execution.wal":
    sys.modules.setdefault("flux.execution.wal", sys.modules[__name__])


OWNERSHIP_WAL_SCHEMA_SQL = """
CREATE TABLE IF NOT EXISTS ownership_wal (
    intent_id TEXT PRIMARY KEY,
    controller_scope_id TEXT NOT NULL,
    strategy_id TEXT NOT NULL,
    controller_epoch INTEGER NOT NULL,
    controller_seq INTEGER NOT NULL,
    client_order_id TEXT NOT NULL UNIQUE,
    venue_order_id TEXT,
    lifecycle_state TEXT NOT NULL,
    materialized_lifecycle_state TEXT,
    materialized_at_ns INTEGER,
    created_at_ns INTEGER NOT NULL,
    updated_at_ns INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS ix_ownership_wal_claim_tuple
ON ownership_wal (
    controller_scope_id,
    controller_epoch,
    controller_seq,
    client_order_id
);

CREATE INDEX IF NOT EXISTS ix_ownership_wal_client_order_id
ON ownership_wal (client_order_id);

CREATE INDEX IF NOT EXISTS ix_ownership_wal_venue_order_id
ON ownership_wal (venue_order_id);
"""


class FenceRejectedError(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class OwnershipWalRecord:
    claim: ExecutionClaim
    venue_order_id: str | None
    lifecycle_state: ExecutionLifecycleState
    materialized_lifecycle_state: ExecutionLifecycleState | None
    materialized_at_ns: int | None
    created_at_ns: int
    updated_at_ns: int


def connect(path: str | Path) -> sqlite3.Connection:
    conn = sqlite3.connect(str(Path(path)), timeout=5.0)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL;")
    conn.execute("PRAGMA synchronous=FULL;")
    return conn


def ensure_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(OWNERSHIP_WAL_SCHEMA_SQL)


def assert_controller_epoch_fence(
    *,
    claim: ExecutionClaim,
    authority: ControllerSnapshotAuthority,
    phase: str,
) -> None:
    if authority.controller_scope_id != claim.controller_scope_id:
        raise FenceRejectedError(f"controller fence rejected {phase}: controller scope mismatch")
    if authority.controller_epoch != claim.controller_epoch:
        raise FenceRejectedError(f"controller fence rejected {phase}: controller epoch mismatch")
    if authority.controller_seq < claim.controller_seq:
        raise FenceRejectedError(f"controller fence rejected {phase}: controller sequence regressed")
    if authority.authority_state is not SnapshotAuthorityState.AUTHORITATIVE:
        raise FenceRejectedError(
            f"controller fence rejected {phase}: authority is {authority.authority_state.value}",
        )


class SQLiteOwnershipWal:
    def __init__(self, *, db_path: str | Path) -> None:
        self._conn = connect(db_path)
        ensure_schema(self._conn)

    @property
    def connection(self) -> sqlite3.Connection:
        return self._conn

    def close(self) -> None:
        self._conn.close()

    def append_claim(
        self,
        *,
        claim: ExecutionClaim,
        authority: ControllerSnapshotAuthority,
        appended_at_ns: int,
    ) -> OwnershipWalRecord:
        assert_controller_epoch_fence(
            claim=claim,
            authority=authority,
            phase="before append",
        )
        timestamp_ns = int(appended_at_ns)
        try:
            with self._conn:
                self._conn.execute(
                    """
                    INSERT INTO ownership_wal (
                        intent_id,
                        controller_scope_id,
                        strategy_id,
                        controller_epoch,
                        controller_seq,
                        client_order_id,
                        venue_order_id,
                        lifecycle_state,
                        materialized_lifecycle_state,
                        materialized_at_ns,
                        created_at_ns,
                        updated_at_ns
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    """,
                    (
                        claim.intent_id,
                        claim.controller_scope_id,
                        claim.strategy_id,
                        claim.controller_epoch,
                        claim.controller_seq,
                        claim.client_order_id,
                        None,
                        ExecutionLifecycleState.OWNED_PRE_WRITE.value,
                        None,
                        None,
                        timestamp_ns,
                        timestamp_ns,
                    ),
                )
        except sqlite3.IntegrityError as exc:
            raise ValueError(f"ownership record already exists for intent_id={claim.intent_id}") from exc
        record = self.fetch_by_intent_id(claim.intent_id)
        if record is None:
            raise RuntimeError(f"missing ownership record after append for intent_id={claim.intent_id}")
        return record

    def record_venue_write(
        self,
        *,
        claim: ExecutionClaim,
        authority: ControllerSnapshotAuthority,
        venue_order_id: str,
        written_at_ns: int,
    ) -> OwnershipWalRecord:
        assert_controller_epoch_fence(
            claim=claim,
            authority=authority,
            phase="before venue write",
        )
        record = self.fetch_by_intent_id(claim.intent_id)
        if record is None:
            raise KeyError(f"missing ownership record for intent_id={claim.intent_id}")
        timestamp_ns = int(written_at_ns)
        venue_order_id = _required_text(venue_order_id, "venue_order_id")
        with self._conn:
            self._conn.execute(
                """
                UPDATE ownership_wal
                SET venue_order_id = ?,
                    lifecycle_state = ?,
                    updated_at_ns = ?
                WHERE intent_id = ?
                """,
                (
                    venue_order_id,
                    ExecutionLifecycleState.SENT_TO_VENUE.value,
                    timestamp_ns,
                    claim.intent_id,
                ),
            )
        updated = self.fetch_by_intent_id(claim.intent_id)
        if updated is None:
            raise RuntimeError(f"missing ownership record after venue write for intent_id={claim.intent_id}")
        return updated

    def update_lifecycle(
        self,
        *,
        intent_id: str,
        lifecycle_state: ExecutionLifecycleState | str,
        updated_at_ns: int,
        venue_order_id: str | None = None,
    ) -> OwnershipWalRecord:
        record = self.fetch_by_intent_id(intent_id)
        if record is None:
            raise KeyError(f"missing ownership record for intent_id={intent_id}")
        next_state = _coerce_lifecycle_state(lifecycle_state)
        next_venue_order_id = venue_order_id or record.venue_order_id
        with self._conn:
            self._conn.execute(
                """
                UPDATE ownership_wal
                SET venue_order_id = ?,
                    lifecycle_state = ?,
                    updated_at_ns = ?
                WHERE intent_id = ?
                """,
                (
                    next_venue_order_id,
                    next_state.value,
                    int(updated_at_ns),
                    intent_id,
                ),
            )
        updated = self.fetch_by_intent_id(intent_id)
        if updated is None:
            raise RuntimeError(f"missing ownership record after lifecycle update for intent_id={intent_id}")
        return updated

    def mark_materialized(
        self,
        *,
        intent_id: str,
        lifecycle_state: ExecutionLifecycleState | str,
        materialized_at_ns: int,
        venue_order_id: str | None = None,
    ) -> OwnershipWalRecord:
        record = self.fetch_by_intent_id(intent_id)
        if record is None:
            raise KeyError(f"missing ownership record for intent_id={intent_id}")
        materialized_state = _coerce_lifecycle_state(lifecycle_state)
        next_venue_order_id = venue_order_id or record.venue_order_id
        timestamp_ns = int(materialized_at_ns)
        with self._conn:
            self._conn.execute(
                """
                UPDATE ownership_wal
                SET venue_order_id = ?,
                    materialized_lifecycle_state = ?,
                    materialized_at_ns = ?,
                    updated_at_ns = ?
                WHERE intent_id = ?
                """,
                (
                    next_venue_order_id,
                    materialized_state.value,
                    timestamp_ns,
                    timestamp_ns,
                    intent_id,
                ),
            )
        updated = self.fetch_by_intent_id(intent_id)
        if updated is None:
            raise RuntimeError(
                f"missing ownership record after materialization mark for intent_id={intent_id}",
            )
        return updated

    def fetch_by_intent_id(self, intent_id: str) -> OwnershipWalRecord | None:
        row = self._conn.execute(
            "SELECT * FROM ownership_wal WHERE intent_id = ?",
            (_required_text(intent_id, "intent_id"),),
        ).fetchone()
        return None if row is None else _record_from_row(row)

    def fetch_by_client_order_id(self, client_order_id: str) -> OwnershipWalRecord | None:
        row = self._conn.execute(
            "SELECT * FROM ownership_wal WHERE client_order_id = ?",
            (_required_text(client_order_id, "client_order_id"),),
        ).fetchone()
        return None if row is None else _record_from_row(row)

    def fetch_by_venue_order_id(self, venue_order_id: str) -> OwnershipWalRecord | None:
        row = self._conn.execute(
            "SELECT * FROM ownership_wal WHERE venue_order_id = ?",
            (_required_text(venue_order_id, "venue_order_id"),),
        ).fetchone()
        return None if row is None else _record_from_row(row)

    def list_records(self) -> list[OwnershipWalRecord]:
        rows = self._conn.execute(
            "SELECT * FROM ownership_wal ORDER BY created_at_ns ASC, intent_id ASC",
        ).fetchall()
        return [_record_from_row(row) for row in rows]


def _coerce_lifecycle_state(
    value: ExecutionLifecycleState | str,
) -> ExecutionLifecycleState:
    if isinstance(value, ExecutionLifecycleState):
        return value
    return ExecutionLifecycleState(_required_text(value, "lifecycle_state"))


def _required_text(value: str | Path, field_name: str) -> str:
    text = str(value).strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    return text


def _record_from_row(row: sqlite3.Row) -> OwnershipWalRecord:
    materialized_state = row["materialized_lifecycle_state"]
    return OwnershipWalRecord(
        claim=ExecutionClaim(
            intent_id=row["intent_id"],
            controller_scope_id=row["controller_scope_id"],
            strategy_id=row["strategy_id"],
            controller_epoch=row["controller_epoch"],
            controller_seq=row["controller_seq"],
            client_order_id=row["client_order_id"],
            venue_order_id=row["venue_order_id"],
            lifecycle_state=ExecutionLifecycleState.ACCEPTED,
        ),
        venue_order_id=row["venue_order_id"],
        lifecycle_state=ExecutionLifecycleState(row["lifecycle_state"]),
        materialized_lifecycle_state=(
            None
            if materialized_state is None
            else ExecutionLifecycleState(materialized_state)
        ),
        materialized_at_ns=row["materialized_at_ns"],
        created_at_ns=row["created_at_ns"],
        updated_at_ns=row["updated_at_ns"],
    )


__all__ = (
    "FenceRejectedError",
    "OWNERSHIP_WAL_SCHEMA_SQL",
    "OwnershipWalRecord",
    "SQLiteOwnershipWal",
    "assert_controller_epoch_fence",
    "connect",
    "ensure_schema",
)
