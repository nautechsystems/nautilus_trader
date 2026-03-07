# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

from collections import OrderedDict
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

from nautilus_trader.persistence._action_intent import CANCEL_INTENT_TYPE
from nautilus_trader.persistence._action_intent import PLACE_INTENT_TYPE
from nautilus_trader.persistence._action_intent import current_ts_ns
from nautilus_trader.persistence._action_intent import iter_json_payload_mappings


EXECUTION_TIMING_TOPIC = "events.execution.timing"
EXECUTION_TIMING_PARAMS_KEY = "nautilus.execution_timing"

PLACE_ACTION_TYPE = PLACE_INTENT_TYPE
CANCEL_ACTION_TYPE = CANCEL_INTENT_TYPE

EXECUTION_TIMING_FIELD_NAMES = (
    "ts_command_init_ns",
    "ts_risk_recv_ns",
    "ts_risk_forward_ns",
    "ts_exec_recv_ns",
    "ts_exec_forward_ns",
    "ts_client_submit_ns",
    "ts_adapter_submit_start_ns",
)
_MUTABLE_EXECUTION_TIMING_FIELDS = frozenset(
    field for field in EXECUTION_TIMING_FIELD_NAMES if field != "ts_command_init_ns"
)


@dataclass(frozen=True, slots=True)
class ExecutionTimingRecord:
    strategy_id: str
    client_order_id: str
    action_type: str
    ts_command_init_ns: int | None
    ts_risk_recv_ns: int | None
    ts_risk_forward_ns: int | None
    ts_exec_recv_ns: int | None
    ts_exec_forward_ns: int | None
    ts_client_submit_ns: int | None
    ts_adapter_submit_start_ns: int | None

    @classmethod
    def from_payload(cls, payload: Mapping[str, Any]) -> ExecutionTimingRecord | None:
        strategy_id = _optional_text(payload.get("strategy_id"))
        client_order_id = _optional_text(payload.get("client_order_id"))
        action_type = _optional_text(payload.get("action_type"))
        if (
            strategy_id is None
            or client_order_id is None
            or action_type not in {PLACE_ACTION_TYPE, CANCEL_ACTION_TYPE}
        ):
            return None

        return cls(
            strategy_id=strategy_id,
            client_order_id=client_order_id,
            action_type=action_type,
            ts_command_init_ns=_optional_int(payload.get("ts_command_init_ns")),
            ts_risk_recv_ns=_optional_int(payload.get("ts_risk_recv_ns")),
            ts_risk_forward_ns=_optional_int(payload.get("ts_risk_forward_ns")),
            ts_exec_recv_ns=_optional_int(payload.get("ts_exec_recv_ns")),
            ts_exec_forward_ns=_optional_int(payload.get("ts_exec_forward_ns")),
            ts_client_submit_ns=_optional_int(payload.get("ts_client_submit_ns")),
            ts_adapter_submit_start_ns=_optional_int(payload.get("ts_adapter_submit_start_ns")),
        )

    def to_payload(self) -> dict[str, Any]:
        return {
            "strategy_id": self.strategy_id,
            "client_order_id": self.client_order_id,
            "action_type": self.action_type,
            "ts_command_init_ns": self.ts_command_init_ns,
            "ts_risk_recv_ns": self.ts_risk_recv_ns,
            "ts_risk_forward_ns": self.ts_risk_forward_ns,
            "ts_exec_recv_ns": self.ts_exec_recv_ns,
            "ts_exec_forward_ns": self.ts_exec_forward_ns,
            "ts_client_submit_ns": self.ts_client_submit_ns,
            "ts_adapter_submit_start_ns": self.ts_adapter_submit_start_ns,
        }


class ExecutionTimingCache:
    def __init__(
        self,
        *,
        max_entries: int = 50_000,
        ttl_ns: int = 24 * 60 * 60 * 1_000_000_000,
    ) -> None:
        self._max_entries = max(1, int(max_entries))
        self._ttl_ns = max(1, int(ttl_ns))
        self._entries: OrderedDict[tuple[str, str, str], tuple[ExecutionTimingRecord, int]] = OrderedDict()

    def add(self, record: ExecutionTimingRecord, *, now_ns: int) -> None:
        self.prune(now_ns=now_ns)
        key = (record.strategy_id, record.client_order_id, record.action_type)
        self._entries.pop(key, None)
        self._entries[key] = (record, now_ns + self._ttl_ns)
        while len(self._entries) > self._max_entries:
            self._entries.popitem(last=False)

    def get(
        self,
        *,
        client_order_id: str,
        action_type: str,
        strategy_id: str | None = None,
        now_ns: int,
    ) -> ExecutionTimingRecord | None:
        if strategy_id is not None:
            key = (strategy_id, client_order_id, action_type)
            cached = self._entries.get(key)
            if cached is None:
                return None
            record, expires_at_ns = cached
            if expires_at_ns <= now_ns:
                self._entries.pop(key, None)
                return None
            return record

        for key, (record, expires_at_ns) in reversed(self._entries.items()):
            if record.client_order_id != client_order_id or record.action_type != action_type:
                continue
            if expires_at_ns <= now_ns:
                self._entries.pop(key, None)
                continue
            return record
        return None

    def evict(self, *, client_order_id: str, strategy_id: str | None = None) -> None:
        self.evict_types(
            client_order_id=client_order_id,
            strategy_id=strategy_id,
            action_types=(PLACE_ACTION_TYPE, CANCEL_ACTION_TYPE),
        )

    def evict_types(
        self,
        *,
        client_order_id: str,
        action_types: tuple[str, ...],
        strategy_id: str | None = None,
    ) -> None:
        if strategy_id is not None:
            for action_type in action_types:
                self._entries.pop((strategy_id, client_order_id, action_type), None)
            return

        for key in tuple(self._entries):
            if key[1] == client_order_id and key[2] in action_types:
                self._entries.pop(key, None)

    def prune(self, *, now_ns: int) -> None:
        while self._entries:
            _, (_, expires_at_ns) = next(iter(self._entries.items()))
            if expires_at_ns > now_ns:
                break
            self._entries.popitem(last=False)

    def clear(self) -> None:
        self._entries.clear()


def record_command_timing(
    command: object,
    *,
    field: str,
    ts_ns: int | None = None,
    clock: object | None = None,
) -> int:
    if field not in _MUTABLE_EXECUTION_TIMING_FIELDS:
        raise ValueError(f"Unsupported execution timing field: {field}")

    timestamp_ns = int(current_ts_ns(clock) if ts_ns is None else ts_ns)
    params = getattr(command, "params", None)
    if not isinstance(params, dict):
        return timestamp_ns

    payload = params.get(EXECUTION_TIMING_PARAMS_KEY)
    if not isinstance(payload, dict):
        payload = {}
        params[EXECUTION_TIMING_PARAMS_KEY] = payload
    payload[field] = timestamp_ns
    return timestamp_ns


def snapshot_command_timing(command: object) -> dict[str, int | None]:
    snapshot = {field: None for field in EXECUTION_TIMING_FIELD_NAMES}
    snapshot["ts_command_init_ns"] = _optional_int(getattr(command, "ts_init", None))

    params = getattr(command, "params", None)
    if not isinstance(params, dict):
        return snapshot

    payload = params.get(EXECUTION_TIMING_PARAMS_KEY)
    if not isinstance(payload, Mapping):
        return snapshot

    for field in EXECUTION_TIMING_FIELD_NAMES:
        if field == "ts_command_init_ns":
            continue
        snapshot[field] = _optional_int(payload.get(field))

    return snapshot


def publish_command_execution_timing(
    msgbus: object | None,
    *,
    command: object,
    client_order_id: str,
    action_type: str,
    strategy_id: str,
    clock: object | None = None,
    topic: str = EXECUTION_TIMING_TOPIC,
    stamp_adapter_submit_start: bool = False,
) -> ExecutionTimingRecord:
    if stamp_adapter_submit_start:
        record_command_timing(
            command,
            field="ts_adapter_submit_start_ns",
            clock=clock,
        )
    snapshot = snapshot_command_timing(command)
    record = ExecutionTimingRecord(
        strategy_id=strategy_id,
        client_order_id=client_order_id,
        action_type=action_type,
        ts_command_init_ns=snapshot["ts_command_init_ns"],
        ts_risk_recv_ns=snapshot["ts_risk_recv_ns"],
        ts_risk_forward_ns=snapshot["ts_risk_forward_ns"],
        ts_exec_recv_ns=snapshot["ts_exec_recv_ns"],
        ts_exec_forward_ns=snapshot["ts_exec_forward_ns"],
        ts_client_submit_ns=snapshot["ts_client_submit_ns"],
        ts_adapter_submit_start_ns=snapshot["ts_adapter_submit_start_ns"],
    )
    if msgbus is not None:
        msgbus.publish(topic=topic, msg=record.to_payload())
    return record


def iter_execution_timing_records(msg: object) -> list[ExecutionTimingRecord]:
    records: list[ExecutionTimingRecord] = []
    for payload in iter_json_payload_mappings(msg):
        record = ExecutionTimingRecord.from_payload(payload)
        if record is not None:
            records.append(record)
    return records


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _optional_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None
