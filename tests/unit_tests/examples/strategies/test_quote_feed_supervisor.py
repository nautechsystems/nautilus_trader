from __future__ import annotations

from flux.runners.shared.quote_feed_supervisor import NodeQuoteFeedSupervisor
from flux.runners.shared.quote_feed_supervisor import QuoteFeedIdentity


def _feed(
    *,
    scope: str = "hyperliquid.xyz.main",
    instrument_id: str = "xyz:AAPL-USD-PERP.HYPERLIQUID",
    topic: str = "maker_quote_ticks",
) -> QuoteFeedIdentity:
    return QuoteFeedIdentity(
        scope=scope,
        instrument_id=instrument_id,
        topic=topic,
    )


def test_quote_feed_supervisor_coalesces_grouped_sibling_recovery_requests() -> None:
    resets: list[str] = []
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.register_claimant(
        feed,
        claimant_id="aapl_tradexyz_maker",
        unusable_after_ms=3_000,
        reset=lambda: resets.append("reset"),
    )
    supervisor.register_claimant(
        feed,
        claimant_id="aapl_tradexyz_taker",
        unusable_after_ms=4_000,
        reset=lambda: resets.append("duplicate"),
    )

    assert supervisor.request_recovery(feed, now_ns=1_000_000_000, requested_by="aapl_tradexyz_maker")
    assert not supervisor.request_recovery(
        feed,
        now_ns=1_000_000_000,
        requested_by="aapl_tradexyz_taker",
    )

    assert resets == ["reset"]
    assert supervisor.snapshot(feed).state == "recovering"


def test_quote_feed_supervisor_keeps_same_instrument_different_feed_identities_distinct() -> None:
    resets: list[str] = []
    supervisor = NodeQuoteFeedSupervisor()
    maker_feed = _feed(topic="maker_quote_ticks")
    reference_feed = _feed(topic="reference_quote_ticks")

    supervisor.register_claimant(
        maker_feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
        reset=lambda: resets.append("maker"),
    )
    supervisor.register_claimant(
        reference_feed,
        claimant_id="maker",
        unusable_after_ms=1_000,
        reset=lambda: resets.append("reference"),
    )

    assert supervisor.request_recovery(maker_feed, now_ns=1_000_000_000, requested_by="maker")
    assert supervisor.request_recovery(reference_feed, now_ns=1_000_000_000, requested_by="maker")

    assert resets == ["maker", "reference"]
    assert supervisor.snapshot(maker_feed).state == "recovering"
    assert supervisor.snapshot(reference_feed).state == "recovering"


def test_quote_feed_supervisor_snapshot_owns_lifecycle_state_fields() -> None:
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
        reset=lambda: None,
    )

    snapshot = supervisor.snapshot(feed)

    assert snapshot.desired is True
    assert snapshot.state == "bootstrapping"
    assert snapshot.attempt_count == 0
    assert snapshot.backoff_until is None
    assert snapshot.last_error_summary is None


def test_quote_feed_supervisor_uses_strictest_active_budget_for_local_usability() -> None:
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=5_000,
        reset=lambda: None,
    )
    supervisor.register_claimant(
        feed,
        claimant_id="taker",
        unusable_after_ms=3_000,
        reset=lambda: None,
    )
    supervisor.record_quote(feed, ts_ns=0)

    assert supervisor.is_locally_usable(feed, now_ns=2_000_000_000)
    assert not supervisor.is_locally_usable(feed, now_ns=4_000_000_000)
    assert supervisor.refresh(feed, now_ns=4_000_000_000).state == "stale"


def test_quote_feed_supervisor_fresh_quote_transitions_bootstrap_and_recovery_back_to_healthy() -> None:
    resets: list[str] = []
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
        reset=lambda: resets.append("reset"),
    )

    assert supervisor.snapshot(feed).state == "bootstrapping"
    supervisor.record_quote(feed, ts_ns=1_000_000_000)
    assert supervisor.snapshot(feed).state == "healthy"

    supervisor.refresh(feed, now_ns=5_000_000_000)
    assert supervisor.request_recovery(feed, now_ns=5_000_000_000, requested_by="maker")
    assert supervisor.snapshot(feed).state == "recovering"

    supervisor.record_quote(feed, ts_ns=6_000_000_000)

    assert resets == ["reset"]
    assert supervisor.snapshot(feed).state == "healthy"


def test_quote_feed_supervisor_successful_recovery_does_not_stick_fresh_feed_in_recovering() -> None:
    resets: list[str] = []
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
        reset=lambda: resets.append("reset"),
    )
    supervisor.record_quote(feed, ts_ns=1_000_000_000)

    assert supervisor.request_recovery(feed, now_ns=2_000_000_000, requested_by="maker")
    snapshot = supervisor.ingest_recovery_result(feed, now_ns=2_000_000_100, ok=True)

    assert resets == ["reset"]
    assert snapshot.state == "healthy"


def test_quote_feed_supervisor_repeated_failed_recovery_transitions_to_down() -> None:
    resets: list[int] = []
    supervisor = NodeQuoteFeedSupervisor(max_attempts=2, recovery_backoff_ns=100)
    feed = _feed()

    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
        reset=lambda: resets.append(1),
    )

    assert supervisor.request_recovery(feed, now_ns=1_000, requested_by="maker")
    supervisor.ingest_recovery_result(feed, now_ns=1_100, ok=False, error_summary="boom-1")
    assert supervisor.snapshot(feed).state == "stale"

    assert supervisor.request_recovery(feed, now_ns=1_250, requested_by="maker")
    supervisor.ingest_recovery_result(feed, now_ns=1_350, ok=False, error_summary="boom-2")

    snapshot = supervisor.snapshot(feed)
    assert resets == [1, 1]
    assert snapshot.state == "down"
    assert snapshot.attempt_count == 2
    assert snapshot.last_error_summary == "boom-2"


def test_quote_feed_supervisor_missing_preconditions_transition_to_blocked_without_reset_storms() -> None:
    resets: list[str] = []
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
        reset=lambda: resets.append("reset"),
        blocker_key="hyperliquid.xyz.main",
    )
    supervisor.set_blocker("hyperliquid.xyz.main", blocked=True, reason="session_down")

    assert not supervisor.request_recovery(feed, now_ns=1_000, requested_by="maker")
    assert not supervisor.request_recovery(feed, now_ns=2_000, requested_by="maker")

    snapshot = supervisor.snapshot(feed)
    assert resets == []
    assert snapshot.state == "blocked"
    assert snapshot.last_error_summary == "session_down"


def test_quote_feed_supervisor_retries_startup_blocked_feed_after_blocker_clears() -> None:
    resets: list[str] = []
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.ensure_feed(
        feed,
        reset=lambda: resets.append("reset"),
        blocker_key="hyperliquid.xyz.main",
    )
    supervisor.set_blocker("hyperliquid.xyz.main", blocked=True, reason="session_down")
    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
        blocker_key="hyperliquid.xyz.main",
    )

    assert supervisor.snapshot(feed).state == "blocked"
    supervisor.set_blocker("hyperliquid.xyz.main", blocked=False, reason=None)
    assert supervisor.should_attempt_recovery(feed, now_ns=1_000)
    assert supervisor.request_recovery(feed, now_ns=1_000, requested_by="maker")
    assert resets == ["reset"]
    assert supervisor.snapshot(feed).state == "recovering"


def test_quote_feed_supervisor_retries_bootstrap_feed_when_first_quote_never_arrives() -> None:
    commands: list[str] = []
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.ensure_feed(
        feed,
        reset=lambda: commands.append("reset"),
        subscribe=lambda: commands.append("subscribe"),
    )
    snapshot = supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
    )

    assert commands == ["subscribe"]
    assert snapshot.state == "bootstrapping"
    assert not supervisor.should_attempt_recovery(feed, now_ns=1_000)
    assert not supervisor.request_recovery(feed, now_ns=1_000, requested_by="maker")
    assert supervisor.should_attempt_recovery(feed, now_ns=3_000_001_000)
    assert supervisor.request_recovery(feed, now_ns=3_000_001_000, requested_by="maker")
    assert commands == ["subscribe", "reset"]
    assert supervisor.snapshot(feed).state == "recovering"


def test_quote_feed_supervisor_failed_startup_recovery_returns_to_retryable_state() -> None:
    commands: list[str] = []
    supervisor = NodeQuoteFeedSupervisor(recovery_backoff_ns=100)
    feed = _feed()

    supervisor.ensure_feed(
        feed,
        reset=lambda: commands.append("reset"),
        subscribe=lambda: commands.append("subscribe"),
    )
    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
    )

    assert not supervisor.should_attempt_recovery(feed, now_ns=1_000)
    assert supervisor.should_attempt_recovery(feed, now_ns=3_000_001_000)
    assert supervisor.request_recovery(feed, now_ns=3_000_001_000, requested_by="maker")

    snapshot = supervisor.ingest_recovery_result(
        feed,
        now_ns=3_000_001_100,
        ok=False,
        error_summary="not_desired",
    )

    assert commands == ["subscribe", "reset"]
    assert snapshot.state == "stale"
    assert snapshot.attempt_count == 1
    assert snapshot.last_error_summary == "not_desired"
    assert supervisor.should_attempt_recovery(feed, now_ns=3_000_001_201)
    assert not supervisor.request_recovery(feed, now_ns=3_000_001_199, requested_by="maker")
    assert supervisor.request_recovery(feed, now_ns=3_000_001_201, requested_by="maker")
    assert commands == ["subscribe", "reset", "reset"]


def test_quote_feed_supervisor_node_local_blocker_suppresses_per_feed_reset_storms() -> None:
    resets: list[str] = []
    supervisor = NodeQuoteFeedSupervisor()
    maker_feed = _feed(instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID")
    hedge_feed = _feed(instrument_id="xyz:MSFT-USD-PERP.HYPERLIQUID")

    for feed in (maker_feed, hedge_feed):
        supervisor.register_claimant(
            feed,
            claimant_id=str(feed.instrument_id),
            unusable_after_ms=3_000,
            reset=lambda feed=feed: resets.append(str(feed.instrument_id)),
            blocker_key="hyperliquid.xyz.main",
        )

    supervisor.set_blocker("hyperliquid.xyz.main", blocked=True, reason="transport_down")

    assert not supervisor.request_recovery(maker_feed, now_ns=1_000, requested_by="maker")
    assert not supervisor.request_recovery(hedge_feed, now_ns=1_000, requested_by="taker")

    assert resets == []
    assert supervisor.snapshot(maker_feed).state == "blocked"
    assert supervisor.snapshot(hedge_feed).state == "blocked"


def test_quote_feed_supervisor_unregister_removes_claimant_budget_influence() -> None:
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=5_000,
        reset=lambda: None,
    )
    supervisor.register_claimant(
        feed,
        claimant_id="taker",
        unusable_after_ms=3_000,
        reset=lambda: None,
    )
    supervisor.record_quote(feed, ts_ns=0)

    assert supervisor.refresh(feed, now_ns=4_000_000_000).state == "stale"

    supervisor.unregister_claimant(feed, claimant_id="taker")

    assert supervisor.refresh(feed, now_ns=4_000_000_000).state == "healthy"


def test_quote_feed_supervisor_remaining_sibling_can_advance_shared_feed_health() -> None:
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
        reset=lambda: None,
    )
    supervisor.register_claimant(
        feed,
        claimant_id="taker",
        unusable_after_ms=3_000,
        reset=lambda: None,
    )

    supervisor.request_recovery(feed, now_ns=1_000, requested_by="maker")
    supervisor.unregister_claimant(feed, claimant_id="maker")
    supervisor.record_quote(feed, ts_ns=2_000)

    snapshot = supervisor.snapshot(feed)
    assert snapshot.state == "healthy"
    assert snapshot.claimant_ids == ("taker",)


def test_quote_feed_supervisor_owns_first_subscribe_and_last_unsubscribe() -> None:
    commands: list[str] = []
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.ensure_feed(
        feed,
        reset=lambda: commands.append("reset"),
        subscribe=lambda: commands.append("subscribe"),
        unsubscribe=lambda: commands.append("unsubscribe"),
    )

    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
    )
    supervisor.register_claimant(
        feed,
        claimant_id="taker",
        unusable_after_ms=3_000,
    )
    supervisor.unregister_claimant(feed, claimant_id="maker")
    supervisor.unregister_claimant(feed, claimant_id="taker")

    assert commands == ["subscribe", "unsubscribe"]


def test_quote_feed_supervisor_resubscribes_after_final_unsubscribe_cycle() -> None:
    commands: list[str] = []
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.ensure_feed(
        feed,
        reset=lambda: commands.append("reset"),
        subscribe=lambda: commands.append("subscribe"),
        unsubscribe=lambda: commands.append("unsubscribe"),
    )
    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
    )
    supervisor.record_quote(feed, ts_ns=1_000)
    supervisor.unregister_claimant(feed, claimant_id="maker")
    supervisor.record_quote(feed, ts_ns=2_000)

    snapshot = supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
    )

    assert commands == ["subscribe", "unsubscribe", "subscribe"]
    assert snapshot.state == "bootstrapping"


def test_quote_feed_supervisor_ignores_late_recovery_result_after_final_unsubscribe() -> None:
    commands: list[str] = []
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed()

    supervisor.ensure_feed(
        feed,
        reset=lambda: commands.append("reset"),
        subscribe=lambda: commands.append("subscribe"),
        unsubscribe=lambda: commands.append("unsubscribe"),
    )
    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
    )

    assert supervisor.request_recovery(feed, now_ns=3_000_001_000, requested_by="maker")
    supervisor.unregister_claimant(feed, claimant_id="maker")

    late_snapshot = supervisor.ingest_recovery_result(
        feed,
        now_ns=3_000_001_100,
        ok=False,
        error_summary="late-boom",
    )

    snapshot = supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=3_000,
    )

    assert commands == ["subscribe", "reset", "unsubscribe", "subscribe"]
    assert late_snapshot.desired is False
    assert late_snapshot.state == "bootstrapping"
    assert late_snapshot.attempt_count == 0
    assert late_snapshot.backoff_until is None
    assert late_snapshot.last_error_summary is None
    assert snapshot.state == "bootstrapping"
    assert snapshot.attempt_count == 0
    assert snapshot.backoff_until is None
    assert snapshot.last_error_summary is None


def test_quote_feed_supervisor_does_not_admit_recovery_without_node_owned_reset() -> None:
    supervisor = NodeQuoteFeedSupervisor()
    feed = _feed(
        scope="ibkr.shared_publisher",
        instrument_id="AAPL.NASDAQ",
        topic="reference_quote_ticks",
    )

    supervisor.ensure_feed(
        feed,
        reset=None,
        blocker_key="ibkr.shared_publisher",
    )
    supervisor.register_claimant(
        feed,
        claimant_id="maker",
        unusable_after_ms=1_000,
        blocker_key="ibkr.shared_publisher",
    )
    supervisor.record_quote(feed, ts_ns=0)

    assert supervisor.refresh(feed, now_ns=2_000_000_000).state == "stale"
    assert not supervisor.request_recovery(feed, now_ns=2_000_000_000, requested_by="maker")
    assert supervisor.snapshot(feed).state == "stale"
