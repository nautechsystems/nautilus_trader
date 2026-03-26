from __future__ import annotations

import asyncio
import copy
import logging
import threading
import time
from contextlib import suppress
from dataclasses import dataclass
from decimal import Decimal
from typing import Any

from flux.api._payloads_balances import build_balances_rows
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers.factories import drop_cached_ib_client
from nautilus_trader.adapters.interactive_brokers.factories import get_cached_ib_client
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.model.identifiers import TraderId


_ACCOUNT_SUMMARY_TAGS = frozenset(
    {
        "NetLiquidation",
        "FullAvailableFunds",
        "FullInitMarginReq",
        "FullMaintMarginReq",
        "TotalCashValue",
    },
)


@dataclass(frozen=True)
class IbkrReferenceBalanceSnapshotProviderConfig:
    ibg_host: str = "127.0.0.1"
    ibg_port: int | None = None
    ibg_client_id: int = 1
    dockerized_gateway: DockerizedIBGatewayConfig | None = None
    connection_timeout: int = 300
    request_timeout_secs: int = 60
    account_id: str | None = None
    refresh_interval_secs: float = 15.0


@dataclass(frozen=True)
class _StandaloneIbkrRuntime:
    msgbus: MessageBus
    cache: Cache
    clock: LiveClock
    log: logging.Logger
    loop: asyncio.AbstractEventLoop
    thread: threading.Thread


class IbkrReferenceBalanceSnapshotProvider:
    def __init__(self, config: IbkrReferenceBalanceSnapshotProviderConfig) -> None:
        self._config = config
        self._task: asyncio.Task | None = None
        self._latest_snapshot: dict[str, Any] | None = None
        self._account_id = config.account_id
        self._attachments = 0
        self._standalone_runtime: _StandaloneIbkrRuntime | None = None
        self._last_refresh_monotonic = 0.0
        self._cached_client_key: tuple[str, int | None, int] | None = None

    def start(self, *, strategy: Any) -> None:
        self._attachments += 1
        if self._task is not None and not self._task.done():
            return
        loop = asyncio.get_running_loop()
        self._task = loop.create_task(self._run(strategy))

    def stop(self) -> None:
        self._attachments = max(0, self._attachments - 1)
        if self._attachments > 0:
            return
        if self._task is not None:
            self._task.cancel()
            self._task = None
        self._stop_standalone_runtime()

    def snapshot(self) -> dict[str, Any] | None:
        if self._latest_snapshot is None:
            return None
        return copy.deepcopy(self._latest_snapshot)

    def refresh(self) -> dict[str, Any] | None:
        if self._task is not None and not self._task.done():
            return self.snapshot()

        now = time.monotonic()
        if (
            self._latest_snapshot is not None
            and (now - self._last_refresh_monotonic) < self._config.refresh_interval_secs
        ):
            return self.snapshot()

        runtime = self._standalone_runtime
        if runtime is None:
            runtime = self._ensure_standalone_runtime()
        attempt_ts_ms = int(time.time() * 1000)
        try:
            future = asyncio.run_coroutine_threadsafe(
                self._fetch_snapshot(runtime),
                runtime.loop,
            )
            snapshot = future.result(
                timeout=self._config.connection_timeout + self._config.request_timeout_secs,
            )
            self._latest_snapshot = self._annotate_refresh_success(
                snapshot,
                attempt_ts_ms=attempt_ts_ms,
            )
            self._last_refresh_monotonic = time.monotonic()
        except Exception as exc:
            self._latest_snapshot = self._annotate_refresh_failure(
                exc,
                attempt_ts_ms=attempt_ts_ms,
            )
            with suppress(Exception):
                runtime.log.warning(
                    "IBKR reference balance refresh failed (%s): %s",
                    type(exc).__name__,
                    exc,
                )
            if self._is_recoverable_refresh_failure(exc):
                self._reset_standalone_client_runtime()
        return self.snapshot()

    def _ensure_standalone_runtime(self) -> _StandaloneIbkrRuntime:
        runtime = self._standalone_runtime
        if (
            runtime is not None
            and runtime.thread.is_alive()
            and not runtime.loop.is_closed()
        ):
            return runtime

        ready = threading.Event()
        holder: dict[str, _StandaloneIbkrRuntime] = {}

        def _run_loop() -> None:
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            clock = LiveClock()
            runtime = _StandaloneIbkrRuntime(
                msgbus=MessageBus(
                    trader_id=TraderId("EQUITIES-ACCOUNT-PROJECTION"),
                    clock=clock,
                ),
                cache=Cache(database=None),
                clock=clock,
                log=logging.getLogger("nautilus-equities-account-projection"),
                loop=loop,
                thread=threading.current_thread(),
            )
            holder["runtime"] = runtime
            ready.set()
            try:
                loop.run_forever()
            finally:
                pending = [task for task in asyncio.all_tasks(loop) if not task.done()]
                for task in pending:
                    task.cancel()
                if pending:
                    loop.run_until_complete(asyncio.gather(*pending, return_exceptions=True))
                loop.close()

        thread = threading.Thread(
            target=_run_loop,
            name=f"ibkr-reference-balance-{self._config.ibg_client_id}",
            daemon=True,
        )
        thread.start()
        if not ready.wait(timeout=5):
            raise RuntimeError("Timed out starting standalone IBKR balance refresh loop")
        runtime = holder["runtime"]
        self._standalone_runtime = runtime
        return runtime

    def _stop_standalone_runtime(self) -> None:
        runtime = self._standalone_runtime
        if runtime is None:
            return
        self._standalone_runtime = None
        loop = getattr(runtime, "loop", None)
        loop_is_closed = getattr(loop, "is_closed", None)
        loop_closed = bool(loop_is_closed()) if callable(loop_is_closed) else False
        loop_stop = getattr(loop, "stop", None)
        loop_call_soon_threadsafe = getattr(loop, "call_soon_threadsafe", None)
        if (
            not loop_closed
            and callable(loop_stop)
            and callable(loop_call_soon_threadsafe)
        ):
            loop_call_soon_threadsafe(loop_stop)
        thread = getattr(runtime, "thread", None)
        thread_is_alive = getattr(thread, "is_alive", None)
        if callable(thread_is_alive) and thread_is_alive():
            thread.join(timeout=5)

    async def _run(self, strategy: Any) -> None:
        try:
            while True:
                attempt_ts_ms = int(strategy.clock.timestamp_ns() // 1_000_000)
                try:
                    snapshot = await self._fetch_snapshot(strategy)
                    self._latest_snapshot = self._annotate_refresh_success(
                        snapshot,
                        attempt_ts_ms=attempt_ts_ms,
                    )
                except asyncio.CancelledError:
                    raise
                except Exception as exc:
                    self._latest_snapshot = self._annotate_refresh_failure(
                        exc,
                        attempt_ts_ms=attempt_ts_ms,
                    )
                    log = getattr(strategy, "log", None)
                    if log is not None:
                        with suppress(Exception):
                            log.warning(
                                "IBKR reference balance refresh failed (%s): %s",
                                type(exc).__name__,
                                exc,
                            )
                await asyncio.sleep(self._config.refresh_interval_secs)
        except asyncio.CancelledError:
            return

    async def _fetch_snapshot(self, strategy: Any) -> dict[str, Any]:
        loop = asyncio.get_running_loop()
        client = get_cached_ib_client(
            loop=loop,
            msgbus=strategy.msgbus,
            cache=strategy.cache,
            clock=strategy.clock,
            host=self._config.ibg_host,
            port=self._config.ibg_port,
            client_id=self._config.ibg_client_id,
            dockerized_gateway=self._config.dockerized_gateway,
            request_timeout_secs=self._config.request_timeout_secs,
        )
        client_host = str(getattr(client, "_host", self._config.ibg_host))
        client_port = getattr(client, "_port", self._config.ibg_port)
        client_id = int(getattr(client, "_client_id", self._config.ibg_client_id))
        self._cached_client_key = (client_host, client_port, client_id)
        await client.wait_until_ready(self._config.connection_timeout)
        account_id = self._resolve_account_id(client)
        ts_ms = int(strategy.clock.timestamp_ns() // 1_000_000)
        account_payload = await self._fetch_account_payload(client, account_id=account_id, ts_ms=ts_ms)
        positions_payload = await self._fetch_positions_payload(
            client,
            account_id=account_id,
            ts_ms=ts_ms,
        )
        payload = {
            "accounts": [account_payload],
            "positions": positions_payload,
        }
        payload["rows"] = build_balances_rows(
            raw_snapshot=payload,
            strategy_id="shared_account",
        )
        return payload

    def _projection_stale_after_ms(self) -> int:
        return max(1_000, int(self._config.refresh_interval_secs * 1_000))

    def _last_success_ts_ms(self) -> int | None:
        snapshot = self._latest_snapshot
        if not isinstance(snapshot, dict):
            return None
        projection_status = snapshot.get("projection_status")
        if isinstance(projection_status, dict):
            value = projection_status.get("last_success_ts_ms")
            if isinstance(value, int):
                return value
        rows = snapshot.get("rows")
        if not isinstance(rows, list):
            return None
        row_ts_values = [
            int(ts_ms)
            for row in rows
            if isinstance(row, dict)
            for ts_ms in [row.get("ts_ms")]
            if isinstance(ts_ms, int)
        ]
        return max(row_ts_values) if row_ts_values else None

    def _annotate_refresh_success(
        self,
        snapshot: dict[str, Any],
        *,
        attempt_ts_ms: int,
    ) -> dict[str, Any]:
        payload = copy.deepcopy(snapshot)
        rows = payload.get("rows")
        if isinstance(rows, list):
            for row in rows:
                if isinstance(row, dict):
                    row.setdefault("stale", False)
                    row.setdefault("include_in_reconciliation", True)
        payload.setdefault("source_scope", "shared_account")
        payload["projection_status"] = {
            "healthy": True,
            "last_success_ts_ms": attempt_ts_ms,
            "last_attempt_ts_ms": attempt_ts_ms,
            "last_error_type": None,
            "last_error_message": None,
            "stale_after_ms": self._projection_stale_after_ms(),
        }
        return payload

    def _annotate_refresh_failure(
        self,
        exc: Exception,
        *,
        attempt_ts_ms: int,
    ) -> dict[str, Any]:
        previous = copy.deepcopy(self._latest_snapshot) if isinstance(self._latest_snapshot, dict) else {}
        rows = previous.get("rows")
        if not isinstance(rows, list):
            rows = []
        stale_after_ms = self._projection_stale_after_ms()
        last_success_ts_ms = self._last_success_ts_ms()
        scope_stale = (
            last_success_ts_ms is None
            or (attempt_ts_ms - last_success_ts_ms) > stale_after_ms
        )
        for row in rows:
            if not isinstance(row, dict):
                continue
            if scope_stale:
                row["stale"] = True
                row["include_in_reconciliation"] = False
            else:
                row.setdefault("stale", False)
                row.setdefault("include_in_reconciliation", True)
        previous["rows"] = rows
        previous.setdefault("source_scope", "shared_account")
        previous["projection_status"] = {
            "healthy": False,
            "last_success_ts_ms": last_success_ts_ms,
            "last_attempt_ts_ms": attempt_ts_ms,
            "last_error_type": type(exc).__name__,
            "last_error_message": str(exc),
            "stale_after_ms": stale_after_ms,
        }
        return previous

    def _is_recoverable_refresh_failure(self, exc: Exception) -> bool:
        if isinstance(exc, (TimeoutError, ConnectionError)):
            return True
        if isinstance(exc, RuntimeError):
            return "loop" in str(exc).lower()
        return False

    def _reset_standalone_client_runtime(self) -> None:
        client_key = self._cached_client_key
        self._cached_client_key = None
        if client_key is not None:
            with suppress(Exception):
                drop_cached_ib_client(
                    host=client_key[0],
                    port=client_key[1],
                    client_id=client_key[2],
                )
        self._stop_standalone_runtime()

    def _resolve_account_id(self, client: Any) -> str:
        if self._account_id:
            return self._account_id
        account_ids = sorted(str(account_id) for account_id in client.accounts())
        if not account_ids:
            raise RuntimeError("IBKR reference balances require at least one managed account")
        if len(account_ids) > 1:
            raise RuntimeError(
                "IBKR reference balances require explicit node.venues.IBKR.account_id when multiple managed accounts are present",
            )
        self._account_id = account_ids[0]
        return self._account_id

    async def _fetch_account_payload(
        self,
        client: Any,
        *,
        account_id: str,
        ts_ms: int,
    ) -> dict[str, Any]:
        summary: dict[str, dict[str, Any]] = {}
        completed = asyncio.Event()
        subscription_name = f"accountSummary-{account_id}"
        completion_name = "accountSummaryEnd"

        def _on_account_summary(tag: str, value: str, currency: str) -> None:
            if not currency:
                return
            bucket = summary.setdefault(str(currency), {})
            try:
                bucket[tag] = Decimal(str(value))
            except Exception:
                bucket[tag] = value

        def _on_account_summary_end(_req_id: int) -> None:
            completed.set()

        client.subscribe_event(subscription_name, _on_account_summary)
        client.subscribe_event(completion_name, _on_account_summary_end)
        try:
            client.subscribe_account_summary()
            await asyncio.wait_for(completed.wait(), timeout=self._config.request_timeout_secs)
        finally:
            with suppress(Exception):
                client.unsubscribe_event(subscription_name)
            with suppress(Exception):
                client.unsubscribe_event(completion_name)

        balances: list[dict[str, str]] = []
        for currency in sorted(summary):
            values = summary[currency]
            total = values.get("NetLiquidation")
            if total is None:
                total = values.get("TotalCashValue")
            free = values.get("FullAvailableFunds")
            if free is None:
                free = values.get("TotalCashValue")
            if total is None or free is None:
                continue
            total_dec = Decimal(str(total))
            free_dec = Decimal(str(free))
            balances.append(
                {
                    "currency": currency,
                    "free": str(free_dec),
                    "locked": str(total_dec - free_dec),
                    "total": str(total_dec),
                },
            )

        return {
            "account_id": account_id,
            "venue": "ibkr",
            "events": [
                {
                    "account_id": account_id,
                    "venue": "ibkr",
                    "balances": balances,
                    "ts_ms": ts_ms,
                },
            ],
        }

    async def _fetch_positions_payload(
        self,
        client: Any,
        *,
        account_id: str,
        ts_ms: int,
    ) -> list[dict[str, Any]]:
        positions = await client.get_positions(account_id)
        payload: list[dict[str, Any]] = []
        for position in positions:
            qty = Decimal(str(position.quantity))
            if qty == 0:
                continue
            instrument_id = self._instrument_id_from_contract(position.contract)
            payload.append(
                {
                    "kind": "position",
                    "exchange": "ibkr",
                    "account_id": position.account_id,
                    "account": position.account_id,
                    "instrument_id": instrument_id,
                    "asset": str(position.contract.symbol).upper(),
                    "signed_qty": str(qty),
                    "quantity": str(abs(qty)),
                    "side": "LONG" if qty > 0 else "SHORT",
                    "avg_px_open": position.avg_cost,
                    "ts_ms": ts_ms,
                },
            )
        return payload

    @staticmethod
    def _instrument_id_from_contract(contract: Any) -> str:
        symbol = str(getattr(contract, "localSymbol", "") or getattr(contract, "symbol", "")).strip()
        exchange = str(
            getattr(contract, "primaryExchange", "") or getattr(contract, "exchange", ""),
        ).strip()
        if exchange:
            return f"{symbol}.{exchange}"
        return symbol


_CACHED_PROVIDERS: dict[IbkrReferenceBalanceSnapshotProviderConfig, IbkrReferenceBalanceSnapshotProvider] = {}


def get_cached_ibkr_reference_balance_provider(
    config: IbkrReferenceBalanceSnapshotProviderConfig,
) -> IbkrReferenceBalanceSnapshotProvider:
    provider = _CACHED_PROVIDERS.get(config)
    if provider is None:
        provider = IbkrReferenceBalanceSnapshotProvider(config)
        _CACHED_PROVIDERS[config] = provider
    return provider


__all__ = [
    "IbkrReferenceBalanceSnapshotProvider",
    "IbkrReferenceBalanceSnapshotProviderConfig",
    "get_cached_ibkr_reference_balance_provider",
]
