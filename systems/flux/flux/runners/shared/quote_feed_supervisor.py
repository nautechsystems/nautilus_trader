from __future__ import annotations

from dataclasses import dataclass
import sys
from typing import Any
from typing import Literal


QuoteFeedState = Literal[
    "bootstrapping",
    "healthy",
    "stale",
    "blocked",
    "recovering",
    "down",
]


@dataclass(frozen=True, slots=True)
class QuoteFeedIdentity:
    scope: str
    instrument_id: Any
    topic: str


@dataclass(frozen=True, slots=True)
class QuoteFeedClaimSpec:
    feed_identity: QuoteFeedIdentity
    claimant_id: str
    unusable_after_ms: int
    blocker_key: str | None = None
    node_scoped_lifecycle: bool = True


@dataclass(frozen=True, slots=True)
class QuoteFeedCommand:
    action: Literal["subscribe", "reset", "unsubscribe"]
    feed_identity: QuoteFeedIdentity
    reason: str | None = None


@dataclass(frozen=True, slots=True)
class QuoteFeedSnapshot:
    desired: bool
    state: QuoteFeedState
    attempt_count: int
    backoff_until: int | None
    last_error_summary: str | None
    claimant_ids: tuple[str, ...]


@dataclass(slots=True)
class _QuoteFeedRecord:
    reset: Any | None
    subscribe: Any | None = None
    unsubscribe: Any | None = None
    blocker_key: str | None = None
    state: QuoteFeedState = "bootstrapping"
    attempt_count: int = 0
    backoff_until: int | None = None
    last_error_summary: str | None = None
    last_quote_ns: int | None = None
    startup_retry_pending: bool = False
    startup_recovery_earliest_ns: int | None = None
    claimants: dict[str, int] | None = None

    def __post_init__(self) -> None:
        if self.claimants is None:
            self.claimants = {}

    @property
    def desired(self) -> bool:
        return bool(self.claimants)

    @property
    def strictest_unusable_after_ns(self) -> int | None:
        if not self.claimants:
            return None
        return min(self.claimants.values()) * 1_000_000


class QuoteFeedControlEmitter:
    def __init__(
        self,
        *,
        node_scoped_id: str,
        sink: Any | None = None,
        result_scheduler: Any | None = None,
    ) -> None:
        self.node_scoped_id = str(node_scoped_id)
        self._sink = sink
        self._result_scheduler = result_scheduler
        self.commands: list[QuoteFeedCommand] = []
        self._result_ingresses: dict[QuoteFeedIdentity, Any] = {}

    def _emit(
        self,
        *,
        action: Literal["subscribe", "reset", "unsubscribe"],
        feed_identity: QuoteFeedIdentity,
        reason: str | None = None,
    ) -> None:
        command = QuoteFeedCommand(
            action=action,
            feed_identity=feed_identity,
            reason=reason,
        )
        self.commands.append(command)
        if callable(self._sink):
            self._sink(command)

    def subscribe(self, feed_identity: QuoteFeedIdentity, *, reason: str | None = None) -> None:
        self._emit(action="subscribe", feed_identity=feed_identity, reason=reason)

    def reset(self, feed_identity: QuoteFeedIdentity, *, reason: str | None = None) -> None:
        self._emit(action="reset", feed_identity=feed_identity, reason=reason)

    def unsubscribe(self, feed_identity: QuoteFeedIdentity, *, reason: str | None = None) -> None:
        self._emit(action="unsubscribe", feed_identity=feed_identity, reason=reason)

    def register_result_ingress(self, feed_identity: QuoteFeedIdentity, ingress: Any) -> None:
        self._result_ingresses[feed_identity] = ingress

    def ingest_result(
        self,
        feed_identity: QuoteFeedIdentity,
        *,
        now_ns: int,
        ok: bool,
        error_summary: str | None = None,
        **extra: Any,
    ) -> Any:
        ingress = self._result_ingresses.get(feed_identity)
        if not callable(ingress):
            return None
        if callable(self._result_scheduler):
            return self._result_scheduler(
                ingress=ingress,
                now_ns=now_ns,
                ok=ok,
                error_summary=error_summary,
                **extra,
            )
        return ingress(
            now_ns=now_ns,
            ok=ok,
            error_summary=error_summary,
            **extra,
        )


class NodeQuoteFeedSupervisor:
    def __init__(
        self,
        *,
        max_attempts: int = 3,
        recovery_backoff_ns: int = 1_000_000_000,
    ) -> None:
        self._max_attempts = max(1, int(max_attempts))
        self._recovery_backoff_ns = max(0, int(recovery_backoff_ns))
        self._feeds: dict[QuoteFeedIdentity, _QuoteFeedRecord] = {}
        self._blockers: dict[str, tuple[bool, str | None]] = {}

    def ensure_feed(
        self,
        feed_identity: QuoteFeedIdentity,
        *,
        reset: Any | None,
        subscribe: Any | None = None,
        unsubscribe: Any | None = None,
        blocker_key: str | None = None,
    ) -> QuoteFeedSnapshot:
        record = self._feeds.get(feed_identity)
        if record is None:
            record = _QuoteFeedRecord(
                reset=reset,
                subscribe=subscribe,
                unsubscribe=unsubscribe,
                blocker_key=blocker_key,
            )
            self._feeds[feed_identity] = record
        elif reset is not None:
            record.reset = reset
        if record.subscribe is None and subscribe is not None:
            record.subscribe = subscribe
        if record.unsubscribe is None and unsubscribe is not None:
            record.unsubscribe = unsubscribe
        if record.blocker_key is None and blocker_key is not None:
            record.blocker_key = blocker_key
        return self.snapshot(feed_identity)

    def peek(self, feed_identity: QuoteFeedIdentity) -> QuoteFeedSnapshot | None:
        if feed_identity not in self._feeds:
            return None
        return self.snapshot(feed_identity)

    def register_claimant(
        self,
        feed_identity: QuoteFeedIdentity,
        *,
        claimant_id: str,
        unusable_after_ms: int,
        now_ns: int | None = None,
        reset: Any | None = None,
        subscribe: Any | None = None,
        unsubscribe: Any | None = None,
        blocker_key: str | None = None,
    ) -> QuoteFeedSnapshot:
        was_desired = bool(self._feeds.get(feed_identity).desired) if feed_identity in self._feeds else False
        record = self._feeds.get(feed_identity)
        if record is None:
            self.ensure_feed(
                feed_identity,
                reset=reset,
                subscribe=subscribe,
                unsubscribe=unsubscribe,
                blocker_key=blocker_key,
            )
            record = self._feeds[feed_identity]
        elif record.reset is None and reset is not None:
            record.reset = reset
        if record.subscribe is None and subscribe is not None:
            record.subscribe = subscribe
        if record.unsubscribe is None and unsubscribe is not None:
            record.unsubscribe = unsubscribe
        if record.blocker_key is None and blocker_key is not None:
            record.blocker_key = blocker_key
        record.claimants[str(claimant_id)] = max(1, int(unusable_after_ms))
        if self._is_blocked(record):
            record.state = "blocked"
            record.last_error_summary = self._blocker_reason(record)
            if record.last_quote_ns is None and callable(record.reset):
                record.startup_retry_pending = True
                record.startup_recovery_earliest_ns = 0
        elif record.last_quote_ns is not None and record.state == "bootstrapping":
            record.state = "healthy"
        elif not was_desired and callable(record.subscribe):
            record.subscribe()
            record.startup_retry_pending = callable(record.reset)
            strictest_ns = record.strictest_unusable_after_ns or 0
            record.startup_recovery_earliest_ns = max(0, int(now_ns or 0)) + strictest_ns
        return self.snapshot(feed_identity)

    def unregister_claimant(
        self,
        feed_identity: QuoteFeedIdentity,
        *,
        claimant_id: str,
    ) -> QuoteFeedSnapshot:
        record = self._feeds[feed_identity]
        was_desired = record.desired
        record.claimants.pop(str(claimant_id), None)
        if not record.desired:
            record.state = "bootstrapping"
            record.attempt_count = 0
            record.backoff_until = None
            record.last_error_summary = None
            record.last_quote_ns = None
            record.startup_retry_pending = False
            record.startup_recovery_earliest_ns = None
            if was_desired and callable(record.unsubscribe):
                record.unsubscribe()
        return self.snapshot(feed_identity)

    def set_blocker(self, blocker_key: str, *, blocked: bool, reason: str | None = None) -> None:
        key = str(blocker_key).strip()
        if not key:
            return
        self._blockers[key] = (bool(blocked), reason)

    def record_quote(self, feed_identity: QuoteFeedIdentity, *, ts_ns: int) -> QuoteFeedSnapshot:
        record = self._feeds[feed_identity]
        if not record.desired:
            return self.snapshot(feed_identity)
        record.last_quote_ns = max(0, int(ts_ns))
        record.backoff_until = None
        record.last_error_summary = None
        record.state = "healthy"
        record.startup_retry_pending = False
        record.startup_recovery_earliest_ns = None
        return self.snapshot(feed_identity)

    def refresh(self, feed_identity: QuoteFeedIdentity, *, now_ns: int) -> QuoteFeedSnapshot:
        record = self._feeds[feed_identity]
        if self._is_blocked(record):
            record.state = "blocked"
            record.last_error_summary = self._blocker_reason(record)
            return self.snapshot(feed_identity)
        if record.state == "recovering":
            return self.snapshot(feed_identity)
        if record.state == "down":
            return self.snapshot(feed_identity)
        if record.last_quote_ns is None:
            if record.attempt_count > 0 or record.backoff_until is not None:
                record.state = "stale"
                return self.snapshot(feed_identity)
            record.state = "bootstrapping"
            return self.snapshot(feed_identity)
        strictest_ns = record.strictest_unusable_after_ns
        if strictest_ns is not None and max(0, int(now_ns)) - record.last_quote_ns > strictest_ns:
            record.state = "stale"
        else:
            record.state = "healthy"
        return self.snapshot(feed_identity)

    def is_locally_usable(self, feed_identity: QuoteFeedIdentity, *, now_ns: int) -> bool:
        return self.refresh(feed_identity, now_ns=now_ns).state == "healthy"

    def should_attempt_recovery(self, feed_identity: QuoteFeedIdentity, *, now_ns: int) -> bool:
        record = self._feeds[feed_identity]
        previous_state = record.state
        snapshot = self.refresh(feed_identity, now_ns=now_ns)
        if snapshot.state == "stale":
            return True
        return (
            snapshot.state == "bootstrapping"
            and record.startup_retry_pending
            and record.desired
            and callable(record.reset)
            and not self._is_blocked(record)
            and self._startup_retry_due(
                record,
                now_ns=now_ns,
                previous_state=previous_state,
            )
        )

    def request_recovery(
        self,
        feed_identity: QuoteFeedIdentity,
        *,
        now_ns: int,
        requested_by: str | None = None,
    ) -> bool:
        del requested_by
        record = self._feeds[feed_identity]
        previous_state = record.state
        self.refresh(feed_identity, now_ns=now_ns)
        if self._is_blocked(record):
            record.state = "blocked"
            return False
        if (
            record.state == "bootstrapping"
            and record.startup_retry_pending
            and not self._startup_retry_due(
                record,
                now_ns=now_ns,
                previous_state=previous_state,
            )
        ):
            return False
        if not callable(record.reset):
            return False
        if record.backoff_until is not None and max(0, int(now_ns)) < record.backoff_until:
            return False
        if record.state == "recovering":
            return False
        if not record.desired:
            return False
        record.state = "recovering"
        record.backoff_until = None
        record.startup_retry_pending = False
        record.startup_recovery_earliest_ns = None
        try:
            if callable(record.reset):
                record.reset()
        except Exception as exc:
            self.ingest_recovery_result(
                feed_identity,
                now_ns=now_ns,
                ok=False,
                error_summary=f"{type(exc).__name__}: {exc}",
            )
            return False
        return True

    def ingest_recovery_result(
        self,
        feed_identity: QuoteFeedIdentity,
        *,
        now_ns: int,
        ok: bool,
        error_summary: str | None = None,
    ) -> QuoteFeedSnapshot:
        record = self._feeds[feed_identity]
        if not record.desired:
            return self.snapshot(feed_identity)
        if ok:
            record.state = "recovering"
            return self.snapshot(feed_identity)
        record.attempt_count += 1
        record.last_error_summary = error_summary
        record.backoff_until = max(0, int(now_ns)) + self._recovery_backoff_ns
        if record.last_quote_ns is None and callable(record.reset):
            record.startup_retry_pending = True
        record.state = "down" if record.attempt_count >= self._max_attempts else "stale"
        return self.snapshot(feed_identity)

    def snapshot(self, feed_identity: QuoteFeedIdentity) -> QuoteFeedSnapshot:
        record = self._feeds[feed_identity]
        return QuoteFeedSnapshot(
            desired=record.desired,
            state=record.state,
            attempt_count=record.attempt_count,
            backoff_until=record.backoff_until,
            last_error_summary=record.last_error_summary,
            claimant_ids=tuple(sorted(record.claimants)),
        )

    def _is_blocked(self, record: _QuoteFeedRecord) -> bool:
        if record.blocker_key is None:
            return False
        blocked, _reason = self._blockers.get(record.blocker_key, (False, None))
        return bool(blocked)

    def _blocker_reason(self, record: _QuoteFeedRecord) -> str | None:
        if record.blocker_key is None:
            return None
        _blocked, reason = self._blockers.get(record.blocker_key, (False, None))
        return reason

    def _startup_retry_due(
        self,
        record: _QuoteFeedRecord,
        *,
        now_ns: int,
        previous_state: QuoteFeedState | None = None,
    ) -> bool:
        if previous_state == "blocked":
            record.startup_recovery_earliest_ns = 0
            return True
        if record.startup_recovery_earliest_ns is None:
            strictest_ns = record.strictest_unusable_after_ns or 0
            record.startup_recovery_earliest_ns = max(0, int(now_ns)) + strictest_ns
            return False
        return max(0, int(now_ns)) >= record.startup_recovery_earliest_ns


if __name__ == "flux.runners.shared.quote_feed_supervisor":
    sys.modules.setdefault(
        "nautilus_trader.flux.runners.shared.quote_feed_supervisor",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.runners.shared.quote_feed_supervisor":
    sys.modules.setdefault("flux.runners.shared.quote_feed_supervisor", sys.modules[__name__])
