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
        try:
            future = asyncio.run_coroutine_threadsafe(
                self._fetch_snapshot(runtime),
                runtime.loop,
            )
            self._latest_snapshot = future.result(
                timeout=self._config.connection_timeout + self._config.request_timeout_secs,
            )
            self._last_refresh_monotonic = time.monotonic()
        except Exception as exc:
            with suppress(Exception):
                runtime.log.warning(f"IBKR reference balance refresh failed: {exc}")
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
        if not runtime.loop.is_closed():
            runtime.loop.call_soon_threadsafe(runtime.loop.stop)
        if runtime.thread.is_alive():
            runtime.thread.join(timeout=5)

    async def _run(self, strategy: Any) -> None:
        try:
            while True:
                try:
                    self._latest_snapshot = await self._fetch_snapshot(strategy)
                except asyncio.CancelledError:
                    raise
                except Exception as exc:
                    log = getattr(strategy, "log", None)
                    if log is not None:
                        with suppress(Exception):
                            log.warning(f"IBKR reference balance refresh failed: {exc}")
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
        loaded = asyncio.Event()
        subscription_name = f"accountSummary-{account_id}"

        def _on_account_summary(tag: str, value: str, currency: str) -> None:
            if not currency:
                return
            bucket = summary.setdefault(str(currency), {})
            try:
                bucket[tag] = Decimal(str(value))
            except Exception:
                bucket[tag] = value
            if _ACCOUNT_SUMMARY_TAGS.issubset(bucket):
                loaded.set()

        client.subscribe_event(subscription_name, _on_account_summary)
        try:
            client.subscribe_account_summary()
            await asyncio.wait_for(loaded.wait(), timeout=self._config.request_timeout_secs)
        finally:
            with suppress(Exception):
                client.unsubscribe_event(subscription_name)

        balances: list[dict[str, str]] = []
        for currency in sorted(summary):
            values = summary[currency]
            total = values.get("NetLiquidation")
            free = values.get("FullAvailableFunds")
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
