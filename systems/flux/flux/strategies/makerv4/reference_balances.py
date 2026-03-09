from __future__ import annotations

import asyncio
import copy
from contextlib import suppress
from dataclasses import dataclass
from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers.factories import get_cached_ib_client


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


class IbkrReferenceBalanceSnapshotProvider:
    def __init__(self, config: IbkrReferenceBalanceSnapshotProviderConfig) -> None:
        self._config = config
        self._task: asyncio.Task | None = None
        self._latest_snapshot: dict[str, Any] | None = None
        self._account_id = config.account_id
        self._attachments = 0

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

    def snapshot(self) -> dict[str, Any] | None:
        if self._latest_snapshot is None:
            return None
        return copy.deepcopy(self._latest_snapshot)

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
        return {
            "accounts": [account_payload],
            "positions": positions_payload,
        }

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
