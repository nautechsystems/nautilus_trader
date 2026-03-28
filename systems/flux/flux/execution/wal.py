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
    wal_seq INTEGER PRIMARY KEY AUTOINCREMENT,
    previous_wal_seq INTEGER,
    intent_id TEXT NOT NULL,
    controller_scope_id TEXT NOT NULL,
    strategy_id TEXT NOT NULL,
    account_scope_id TEXT NOT NULL,
    controller_epoch INTEGER NOT NULL,
    controller_seq INTEGER NOT NULL,
    client_order_id TEXT NOT NULL,
    claim_key TEXT NOT NULL,
    operation_type TEXT NOT NULL,
    venue_order_id TEXT,
    lifecycle_state TEXT NOT NULL,
    materialized_lifecycle_state TEXT,
    materialized_at_ns INTEGER,
    created_at_ns INTEGER NOT NULL,
    updated_at_ns INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_ownership_wal_intent_id
ON ownership_wal (intent_id, wal_seq DESC);

CREATE INDEX IF NOT EXISTS ix_ownership_wal_client_order_id
ON ownership_wal (client_order_id, wal_seq DESC);

CREATE INDEX IF NOT EXISTS ix_ownership_wal_venue_order_id
ON ownership_wal (venue_order_id, wal_seq DESC);

CREATE INDEX IF NOT EXISTS ix_ownership_wal_claim_key
ON ownership_wal (claim_key, wal_seq DESC);
"""


class FenceRejectedError(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class OwnershipWalRecord:
    wal_seq: int
    previous_wal_seq: int | None
    claim: ExecutionClaim
    account_scope_id: str
    operation_type: str
    claim_key: str
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
        account_scope_id: str,
        operation_type: str,
        claim_key: str,
        authority: ControllerSnapshotAuthority,
        appended_at_ns: int,
    ) -> OwnershipWalRecord:
        assert_controller_epoch_fence(
            claim=claim,
            authority=authority,
            phase="before append",
        )
        if self.fetch_by_intent_id(claim.intent_id) is not None:
            raise ValueError(f"ownership record already exists for intent_id={claim.intent_id}")
        return self._insert_record(
            claim=claim,
            account_scope_id=_required_text(account_scope_id, "account_scope_id"),
            operation_type=_required_text(operation_type, "operation_type"),
            claim_key=_required_text(claim_key, "claim_key"),
            venue_order_id=None,
            lifecycle_state=ExecutionLifecycleState.OWNED_PRE_WRITE,
            materialized_lifecycle_state=None,
            materialized_at_ns=None,
            created_at_ns=int(appended_at_ns),
            previous_wal_seq=None,
        )

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
        previous = self.fetch_by_intent_id(claim.intent_id)
        if previous is None:
            raise KeyError(f"missing ownership record for intent_id={claim.intent_id}")
        return self._append_record(
            previous=previous,
            lifecycle_state=ExecutionLifecycleState.SENT_TO_VENUE,
            venue_order_id=_required_text(venue_order_id, "venue_order_id"),
            materialized_lifecycle_state=previous.materialized_lifecycle_state,
            materialized_at_ns=previous.materialized_at_ns,
            created_at_ns=int(written_at_ns),
        )

    def update_lifecycle(
        self,
        *,
        intent_id: str,
        lifecycle_state: ExecutionLifecycleState | str,
        updated_at_ns: int,
        venue_order_id: str | None = None,
    ) -> OwnershipWalRecord:
        previous = self.fetch_by_intent_id(intent_id)
        if previous is None:
            raise KeyError(f"missing ownership record for intent_id={intent_id}")
        return self._append_record(
            previous=previous,
            lifecycle_state=_coerce_lifecycle_state(lifecycle_state),
            venue_order_id=venue_order_id or previous.venue_order_id,
            materialized_lifecycle_state=previous.materialized_lifecycle_state,
            materialized_at_ns=previous.materialized_at_ns,
            created_at_ns=int(updated_at_ns),
        )

    def mark_materialized(
        self,
        *,
        intent_id: str,
        lifecycle_state: ExecutionLifecycleState | str,
        materialized_at_ns: int,
        venue_order_id: str | None = None,
    ) -> OwnershipWalRecord:
        previous = self.fetch_by_intent_id(intent_id)
        if previous is None:
            raise KeyError(f"missing ownership record for intent_id={intent_id}")
        timestamp_ns = int(materialized_at_ns)
        return self._append_record(
            previous=previous,
            lifecycle_state=previous.lifecycle_state,
            venue_order_id=venue_order_id or previous.venue_order_id,
            materialized_lifecycle_state=_coerce_lifecycle_state(lifecycle_state),
            materialized_at_ns=timestamp_ns,
            created_at_ns=timestamp_ns,
        )

    def fetch_by_intent_id(self, intent_id: str) -> OwnershipWalRecord | None:
        return self._fetch_latest(
            "intent_id = ?",
            (_required_text(intent_id, "intent_id"),),
        )

    def fetch_by_client_order_id(self, client_order_id: str) -> OwnershipWalRecord | None:
        return self._fetch_latest(
            "client_order_id = ?",
            (_required_text(client_order_id, "client_order_id"),),
        )

    def fetch_by_venue_order_id(self, venue_order_id: str) -> OwnershipWalRecord | None:
        return self._fetch_latest(
            "venue_order_id = ?",
            (_required_text(venue_order_id, "venue_order_id"),),
        )

    def list_records(self) -> list[OwnershipWalRecord]:
        rows = self._conn.execute(
            "SELECT * FROM ownership_wal ORDER BY wal_seq ASC",
        ).fetchall()
        return [_record_from_row(row) for row in rows]

    def _append_record(
        self,
        *,
        previous: OwnershipWalRecord,
        lifecycle_state: ExecutionLifecycleState,
        venue_order_id: str | None,
        materialized_lifecycle_state: ExecutionLifecycleState | None,
        materialized_at_ns: int | None,
        created_at_ns: int,
    ) -> OwnershipWalRecord:
        return self._insert_record(
            claim=ExecutionClaim(
                intent_id=previous.claim.intent_id,
                controller_scope_id=previous.claim.controller_scope_id,
                strategy_id=previous.claim.strategy_id,
                controller_epoch=previous.claim.controller_epoch,
                controller_seq=previous.claim.controller_seq,
                client_order_id=previous.claim.client_order_id,
                venue_order_id=venue_order_id,
                lifecycle_state=previous.claim.lifecycle_state,
            ),
            account_scope_id=previous.account_scope_id,
            operation_type=previous.operation_type,
            claim_key=previous.claim_key,
            venue_order_id=venue_order_id,
            lifecycle_state=lifecycle_state,
            materialized_lifecycle_state=materialized_lifecycle_state,
            materialized_at_ns=materialized_at_ns,
            created_at_ns=created_at_ns,
            previous_wal_seq=previous.wal_seq,
        )

    def _insert_record(
        self,
        *,
        claim: ExecutionClaim,
        account_scope_id: str,
        operation_type: str,
        claim_key: str,
        venue_order_id: str | None,
        lifecycle_state: ExecutionLifecycleState,
        materialized_lifecycle_state: ExecutionLifecycleState | None,
        materialized_at_ns: int | None,
        created_at_ns: int,
        previous_wal_seq: int | None,
    ) -> OwnershipWalRecord:
        timestamp_ns = int(created_at_ns)
        with self._conn:
            cursor = self._conn.execute(
                """
                INSERT INTO ownership_wal (
                    previous_wal_seq,
                    intent_id,
                    controller_scope_id,
                    strategy_id,
                    account_scope_id,
                    controller_epoch,
                    controller_seq,
                    client_order_id,
                    claim_key,
                    operation_type,
                    venue_order_id,
                    lifecycle_state,
                    materialized_lifecycle_state,
                    materialized_at_ns,
                    created_at_ns,
                    updated_at_ns
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    previous_wal_seq,
                    claim.intent_id,
                    claim.controller_scope_id,
                    claim.strategy_id,
                    account_scope_id,
                    claim.controller_epoch,
                    claim.controller_seq,
                    claim.client_order_id,
                    claim_key,
                    operation_type,
                    venue_order_id,
                    lifecycle_state.value,
                    None if materialized_lifecycle_state is None else materialized_lifecycle_state.value,
                    materialized_at_ns,
                    timestamp_ns,
                    timestamp_ns,
                ),
            )
        record = self._fetch_by_wal_seq(int(cursor.lastrowid))
        if record is None:
            raise RuntimeError(f"missing ownership record after append for intent_id={claim.intent_id}")
        return record

    def _fetch_latest(
        self,
        predicate: str,
        params: tuple[object, ...],
    ) -> OwnershipWalRecord | None:
        row = self._conn.execute(
            f"SELECT * FROM ownership_wal WHERE {predicate} ORDER BY wal_seq DESC LIMIT 1",
            params,
        ).fetchone()
        return None if row is None else _record_from_row(row)

    def _fetch_by_wal_seq(self, wal_seq: int) -> OwnershipWalRecord | None:
        row = self._conn.execute(
            "SELECT * FROM ownership_wal WHERE wal_seq = ?",
            (int(wal_seq),),
        ).fetchone()
        return None if row is None else _record_from_row(row)


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
        wal_seq=row["wal_seq"],
        previous_wal_seq=row["previous_wal_seq"],
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
        account_scope_id=row["account_scope_id"],
        operation_type=row["operation_type"],
        claim_key=row["claim_key"],
        venue_order_id=row["venue_order_id"],
        lifecycle_state=ExecutionLifecycleState(row["lifecycle_state"]),
        materialized_lifecycle_state=(
            None if materialized_state is None else ExecutionLifecycleState(materialized_state)
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
