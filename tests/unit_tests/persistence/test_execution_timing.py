import pytest

from nautilus_trader.persistence._execution_timing import CANCEL_ACTION_TYPE
from nautilus_trader.persistence._execution_timing import EXECUTION_TIMING_PARAMS_KEY
from nautilus_trader.persistence._execution_timing import ExecutionTimingCache
from nautilus_trader.persistence._execution_timing import ExecutionTimingRecord
from nautilus_trader.persistence._execution_timing import PLACE_ACTION_TYPE
from nautilus_trader.persistence._execution_timing import current_ts_ns
from nautilus_trader.persistence._execution_timing import record_command_timing
from nautilus_trader.persistence._execution_timing import snapshot_command_timing


class _Command:
    def __init__(self, *, ts_init: int, params: dict | None = None) -> None:
        self.ts_init = ts_init
        self.params = {} if params is None else params


def _timing_record(*, client_order_id: str, action_type: str = PLACE_ACTION_TYPE) -> ExecutionTimingRecord:
    return ExecutionTimingRecord(
        strategy_id="STRAT-001",
        client_order_id=client_order_id,
        action_type=action_type,
        ts_command_init_ns=100,
        ts_risk_recv_ns=110,
        ts_risk_forward_ns=120,
        ts_exec_recv_ns=130,
        ts_exec_forward_ns=140,
        ts_client_submit_ns=150,
        ts_adapter_submit_start_ns=160,
    )


def test_execution_timing_record_from_payload_requires_strategy_order_and_action_type() -> None:
    payload = {
        "strategy_id": "STRAT-001",
        "client_order_id": "CLIENT-001",
        "action_type": "PLACE",
        "ts_command_init_ns": 100,
        "ts_risk_recv_ns": 110,
        "ts_exec_recv_ns": 130,
    }

    record = ExecutionTimingRecord.from_payload(payload)

    assert record == ExecutionTimingRecord(
        strategy_id="STRAT-001",
        client_order_id="CLIENT-001",
        action_type=PLACE_ACTION_TYPE,
        ts_command_init_ns=100,
        ts_risk_recv_ns=110,
        ts_risk_forward_ns=None,
        ts_exec_recv_ns=130,
        ts_exec_forward_ns=None,
        ts_client_submit_ns=None,
        ts_adapter_submit_start_ns=None,
    )
    assert ExecutionTimingRecord.from_payload({"strategy_id": "STRAT-001"}) is None


def test_execution_timing_cache_prune_evicts_expired_entries_even_after_reads() -> None:
    cache = ExecutionTimingCache(max_entries=10, ttl_ns=10)
    first = _timing_record(client_order_id="CLIENT-1")
    second = _timing_record(client_order_id="CLIENT-2", action_type=CANCEL_ACTION_TYPE)

    cache.add(first, now_ns=0)
    cache.add(second, now_ns=5)

    assert cache.get(
        client_order_id="CLIENT-1",
        action_type=PLACE_ACTION_TYPE,
        strategy_id="STRAT-001",
        now_ns=6,
    ) == first

    cache.prune(now_ns=12)

    assert ("STRAT-001", "CLIENT-1", PLACE_ACTION_TYPE) not in cache._entries
    assert ("STRAT-001", "CLIENT-2", CANCEL_ACTION_TYPE) in cache._entries
    assert len(cache._entries) == 1


def test_execution_timing_cache_isolates_same_client_order_id_across_strategies() -> None:
    cache = ExecutionTimingCache(max_entries=10, ttl_ns=10)
    first = _timing_record(client_order_id="CLIENT-1")
    second = ExecutionTimingRecord(
        strategy_id="STRAT-002",
        client_order_id="CLIENT-1",
        action_type=PLACE_ACTION_TYPE,
        ts_command_init_ns=200,
        ts_risk_recv_ns=210,
        ts_risk_forward_ns=220,
        ts_exec_recv_ns=230,
        ts_exec_forward_ns=240,
        ts_client_submit_ns=250,
        ts_adapter_submit_start_ns=260,
    )

    cache.add(first, now_ns=0)
    cache.add(second, now_ns=1)

    assert cache.get(
        client_order_id="CLIENT-1",
        action_type=PLACE_ACTION_TYPE,
        strategy_id="STRAT-001",
        now_ns=2,
    ) == first
    assert cache.get(
        client_order_id="CLIENT-1",
        action_type=PLACE_ACTION_TYPE,
        strategy_id="STRAT-002",
        now_ns=2,
    ) == second


def test_record_command_timing_stores_stage_values_under_reserved_params_key() -> None:
    command = _Command(ts_init=1_000)

    record_command_timing(command, field="ts_risk_recv_ns", ts_ns=2_000)
    record_command_timing(command, field="ts_exec_recv_ns", ts_ns=3_000)

    assert command.params[EXECUTION_TIMING_PARAMS_KEY] == {
        "ts_risk_recv_ns": 2_000,
        "ts_exec_recv_ns": 3_000,
    }


def test_record_command_timing_rejects_unknown_stage_names() -> None:
    command = _Command(ts_init=1_000)

    with pytest.raises(ValueError, match="Unsupported execution timing field"):
        record_command_timing(command, field="ts_not_a_real_stage", ts_ns=2_000)


def test_snapshot_command_timing_reads_ts_init_and_reserved_params_payload() -> None:
    command = _Command(
        ts_init=1_000,
        params={
            EXECUTION_TIMING_PARAMS_KEY: {
                "ts_risk_recv_ns": 2_000,
                "ts_risk_forward_ns": 2_100,
                "ts_exec_recv_ns": 3_000,
                "ts_exec_forward_ns": 3_100,
                "ts_client_submit_ns": 4_000,
            },
        },
    )

    snapshot = snapshot_command_timing(command)

    assert snapshot == {
        "ts_command_init_ns": 1_000,
        "ts_risk_recv_ns": 2_000,
        "ts_risk_forward_ns": 2_100,
        "ts_exec_recv_ns": 3_000,
        "ts_exec_forward_ns": 3_100,
        "ts_client_submit_ns": 4_000,
        "ts_adapter_submit_start_ns": None,
    }
