from __future__ import annotations

import sqlite3
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from nautilus_trader.persistence._action_intent import iter_json_payload_mappings
from nautilus_trader.persistence._async_sqlite import _AsyncSQLitePersistenceActor

from flux.persistence.quote_cycles.config import QuoteCyclePersistenceActorConfig
from flux.persistence.quote_cycles.sqlite import QuoteCycleRow
from flux.persistence.quote_cycles.sqlite import connect
from flux.persistence.quote_cycles.sqlite import ensure_schema
from flux.persistence.quote_cycles.sqlite import insert_many
from flux.persistence.quote_cycles.sqlite import quote_cycle_to_row


@dataclass(frozen=True, slots=True)
class _QuoteCycleEnvelope:
    payload: dict[str, Any]


class QuoteCyclePersistenceActor(_AsyncSQLitePersistenceActor[_QuoteCycleEnvelope, QuoteCycleRow]):
    """
    Persist MakerV3 quote-cycle observability events into SQLite.
    """

    def __init__(
        self,
        config: QuoteCyclePersistenceActorConfig,
        *,
        connect_fn: Callable[[str], sqlite3.Connection] = connect,
        ensure_schema_fn: Callable[[sqlite3.Connection], None] = ensure_schema,
        insert_many_fn: Callable[[sqlite3.Connection, list[QuoteCycleRow]], tuple[int, int]] = insert_many,
        run_writer_thread: bool = True,
    ) -> None:
        super().__init__(
            config,
            connect_fn=connect_fn,
            ensure_schema_fn=ensure_schema_fn,
            insert_rows_fn=insert_many_fn,
            run_writer_thread=run_writer_thread,
            thread_name_suffix="quote-cycles",
            writer_name="Quote cycle",
            queue_item_name="quote_cycle",
        )
        self.filtered = 0

    def on_start(self) -> None:
        super().on_start()
        self.msgbus.subscribe(topic=self.config.topic, handler=self._on_event_message)

    def on_stop(self) -> None:
        if self.msgbus is not None:
            self.msgbus.unsubscribe(topic=self.config.topic, handler=self._on_event_message)
        super().on_stop()

    def _on_event_message(self, msg: object) -> None:
        matched = False
        for payload in iter_json_payload_mappings(msg):
            if payload.get("event") != "quote_cycle":
                continue
            matched = True
            self._enqueue_payload(_QuoteCycleEnvelope(payload=payload))
        if not matched:
            self.filtered += 1

    def _build_row(self, payload: _QuoteCycleEnvelope) -> QuoteCycleRow | None:
        trader_id = self.msgbus.trader_id.value if self.msgbus is not None else ""
        return quote_cycle_to_row(payload.payload, trader_id=trader_id)
