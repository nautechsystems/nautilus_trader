from nautilus_trader.persistence._action_intent import ActionIntentCache
from nautilus_trader.persistence._action_intent import ActionIntentRecord
from nautilus_trader.persistence._action_intent import CANCEL_INTENT_TYPE
from nautilus_trader.persistence._action_intent import PLACE_INTENT_TYPE
from nautilus_trader.persistence._action_intent import intent_types_to_evict_for_order_event


def _intent(*, client_order_id: str) -> ActionIntentRecord:
    return ActionIntentRecord(
        strategy_id="STRAT-001",
        client_order_id=client_order_id,
        intent_type=PLACE_INTENT_TYPE,
        run_id="run-1",
        quote_cycle_id="run-1:1",
        reason_code="place_missing_level",
        level_index=0,
        target_px="100.0",
        cancel_px="100.1",
        match_tol="0.01",
        ts_market_data_event_ns=100,
        ts_market_data_recv_ns=101,
        ts_decision_ns=102,
        ts_submit_local_ns=103,
        ts_cancel_request_local_ns=None,
        decision_context_json="null",
    )


def test_action_intent_cache_prune_evicts_expired_entries_even_after_cache_reads() -> None:
    cache = ActionIntentCache(max_entries=10, ttl_ns=10)
    first = _intent(client_order_id="CLIENT-1")
    second = _intent(client_order_id="CLIENT-2")

    cache.add(first, now_ns=0)
    cache.add(second, now_ns=5)

    assert cache.get(
        client_order_id="CLIENT-1",
        intent_type=PLACE_INTENT_TYPE,
        strategy_id="STRAT-001",
        now_ns=6,
    ) == first

    cache.prune(now_ns=12)

    assert ("STRAT-001", "CLIENT-1", PLACE_INTENT_TYPE) not in cache._entries
    assert ("STRAT-001", "CLIENT-2", PLACE_INTENT_TYPE) in cache._entries
    assert len(cache._entries) == 1


def test_action_intent_cache_isolates_same_client_order_id_across_strategies() -> None:
    cache = ActionIntentCache(max_entries=10, ttl_ns=10)
    first = _intent(client_order_id="CLIENT-1")
    second = ActionIntentRecord(
        strategy_id="STRAT-002",
        client_order_id="CLIENT-1",
        intent_type=PLACE_INTENT_TYPE,
        run_id="run-2",
        quote_cycle_id="run-2:1",
        reason_code="place_missing_level",
        level_index=1,
        target_px="200.0",
        cancel_px="200.1",
        match_tol="0.02",
        ts_market_data_event_ns=200,
        ts_market_data_recv_ns=201,
        ts_decision_ns=202,
        ts_submit_local_ns=203,
        ts_cancel_request_local_ns=None,
        decision_context_json="null",
    )

    cache.add(first, now_ns=0)
    cache.add(second, now_ns=1)

    assert cache.get(
        client_order_id="CLIENT-1",
        intent_type=PLACE_INTENT_TYPE,
        strategy_id="STRAT-001",
        now_ns=2,
    ) == first
    assert cache.get(
        client_order_id="CLIENT-1",
        intent_type=PLACE_INTENT_TYPE,
        strategy_id="STRAT-002",
        now_ns=2,
    ) == second


def test_intent_types_to_evict_for_order_event_keeps_place_intent_after_cancel_reject() -> None:
    assert intent_types_to_evict_for_order_event("OrderRejected") == (PLACE_INTENT_TYPE,)
    assert intent_types_to_evict_for_order_event("OrderCanceled") == (
        PLACE_INTENT_TYPE,
        CANCEL_INTENT_TYPE,
    )
    assert intent_types_to_evict_for_order_event("OrderCancelRejected") == (CANCEL_INTENT_TYPE,)
