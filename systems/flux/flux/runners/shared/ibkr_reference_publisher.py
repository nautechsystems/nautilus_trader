from __future__ import annotations

import asyncio
import json
import logging
import math
import sys
import threading
import time
from collections.abc import Mapping
from contextlib import suppress
from dataclasses import dataclass
from datetime import datetime
from datetime import time as dt_time
from datetime import timezone
from typing import Any
from zoneinfo import ZoneInfo

from ibapi.ticktype import TickTypeEnum

from flux.common.account_scopes import decode_account_scopes
from flux.common.keys import FluxRedisKeys
from flux.common.strategy_contracts import decode_strategy_contracts
from nautilus_trader.adapters.interactive_brokers.client.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.model.identifiers import TraderId


if __name__ == "flux.runners.shared.ibkr_reference_publisher":
    sys.modules.setdefault(
        "nautilus_trader.flux.runners.shared.ibkr_reference_publisher",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.runners.shared.ibkr_reference_publisher":
    sys.modules.setdefault("flux.runners.shared.ibkr_reference_publisher", sys.modules[__name__])


_LOG = logging.getLogger("nautilus-equities-ibkr-reference-publisher")
_ET = ZoneInfo("America/New_York")
_PRE_START = dt_time(4, 0)
_RTH_START = dt_time(9, 30)
_RTH_END = dt_time(16, 0)
_POST_END = dt_time(20, 0)
_OVERNIGHT_START = dt_time(20, 0)
_OVERNIGHT_END = dt_time(3, 50)


@dataclass(frozen=True, slots=True)
class IbkrReferenceInstrument:
    instrument_id: str
    symbol: str
    primary_exchange: str


@dataclass(frozen=True, slots=True)
class IbkrReferencePublisherConfig:
    enabled: bool
    profile_id: str
    account_scope_id: str
    service_id: str
    ibg_host: str
    ibg_port: int | None
    ibg_fallback_ports: tuple[int, ...]
    ibg_client_id: int
    connection_timeout_secs: int
    request_timeout_secs: int
    snapshot_interval_ms: int
    stale_after_ms: int
    non_rth_stale_after_ms: int
    reconnect_backoff_initial_ms: int
    reconnect_backoff_max_ms: int
    dockerized_gateway: dict[str, Any] | None
    instruments: tuple[IbkrReferenceInstrument, ...]


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _positive_int(value: Any, *, field_name: str, default: int) -> int:
    raw = default if value is None else value
    if isinstance(raw, bool) or not isinstance(raw, int):
        raise TypeError(f"`{field_name}` must be an integer")
    if raw <= 0:
        raise ValueError(f"`{field_name}` must be > 0")
    return raw


def _reference_scope_id(
    contracts: tuple[Any, ...],
    *,
    override_scope_id: str | None,
) -> str:
    if override_scope_id:
        return override_scope_id

    discovered = {
        contract.reference_account_scope_id
        for contract in contracts
    }
    if not discovered:
        raise ValueError("IBKR reference publisher requires at least one strategy contract")
    if len(discovered) != 1:
        raise ValueError(
            "IBKR reference publisher requires one unique reference_account_scope_id unless "
            "`ibkr_reference_publisher.account_scope_id` is configured explicitly",
        )
    return next(iter(discovered))


def _parse_reference_instrument_id(instrument_id: str) -> tuple[str, str]:
    text = instrument_id.strip().upper()
    if "." not in text:
        raise ValueError(
            f"Unsupported IBKR reference instrument_id {instrument_id!r}; expected <SYMBOL>.<VENUE>",
        )
    symbol, primary_exchange = text.rsplit(".", maxsplit=1)
    if not symbol or not primary_exchange:
        raise ValueError(
            f"Unsupported IBKR reference instrument_id {instrument_id!r}; expected <SYMBOL>.<VENUE>",
        )
    return symbol, primary_exchange


def build_ibkr_reference_publisher_config(config: Mapping[str, Any]) -> IbkrReferencePublisherConfig:
    publisher_cfg = dict(config.get("ibkr_reference_publisher") or {})
    strategy_contracts = decode_strategy_contracts(config.get("strategy_contracts") or [])
    account_scopes = decode_account_scopes(config.get("account_scopes") or [])

    reference_scope_id = _reference_scope_id(
        strategy_contracts,
        override_scope_id=_optional_text(publisher_cfg.get("account_scope_id")),
    )
    scope = next(
        (candidate for candidate in account_scopes if candidate.scope_id == reference_scope_id),
        None,
    )
    if scope is None:
        raise ValueError(
            f"IBKR reference publisher scope {reference_scope_id!r} was not found in [[account_scopes]]",
        )
    if scope.provider.lower() != "ibkr":
        raise ValueError(
            f"IBKR reference publisher scope {reference_scope_id!r} must use provider `ibkr`",
        )

    instrument_ids = sorted(
        {
            contract.reference_instrument_id
            for contract in strategy_contracts
            if contract.reference_account_scope_id == reference_scope_id
        },
    )
    if not instrument_ids:
        raise ValueError(
            f"IBKR reference publisher scope {reference_scope_id!r} has no reference instruments",
        )

    instruments = []
    for instrument_id in instrument_ids:
        symbol, primary_exchange = _parse_reference_instrument_id(instrument_id)
        instruments.append(
            IbkrReferenceInstrument(
                instrument_id=instrument_id,
                symbol=symbol,
                primary_exchange=primary_exchange,
            ),
        )

    reconnect_backoff_initial_ms = _positive_int(
        publisher_cfg.get("reconnect_backoff_initial_ms"),
        field_name="ibkr_reference_publisher.reconnect_backoff_initial_ms",
        default=1_000,
    )
    reconnect_backoff_max_ms = _positive_int(
        publisher_cfg.get("reconnect_backoff_max_ms"),
        field_name="ibkr_reference_publisher.reconnect_backoff_max_ms",
        default=15_000,
    )
    reconnect_backoff_max_ms = max(reconnect_backoff_initial_ms, reconnect_backoff_max_ms)
    stale_after_ms = _positive_int(
        publisher_cfg.get("stale_after_ms"),
        field_name="ibkr_reference_publisher.stale_after_ms",
        default=1_500,
    )

    return IbkrReferencePublisherConfig(
        enabled=bool(publisher_cfg.get("enabled", True)),
        profile_id=_optional_text(publisher_cfg.get("profile_id")) or "equities",
        account_scope_id=reference_scope_id,
        service_id=_optional_text(publisher_cfg.get("service_id")) or "ibkr_reference_publisher",
        ibg_host=scope.ibg_host or "127.0.0.1",
        ibg_port=scope.ibg_port,
        ibg_fallback_ports=scope.ibg_fallback_ports,
        ibg_client_id=_positive_int(
            publisher_cfg.get("ibg_client_id"),
            field_name="ibkr_reference_publisher.ibg_client_id",
            default=1 if scope.ibg_client_id is None else scope.ibg_client_id,
        ),
        connection_timeout_secs=max(1, scope.ibg_connection_timeout_secs or 5),
        request_timeout_secs=max(1, scope.ibg_request_timeout_secs or 10),
        snapshot_interval_ms=_positive_int(
            publisher_cfg.get("snapshot_interval_ms"),
            field_name="ibkr_reference_publisher.snapshot_interval_ms",
            default=200,
        ),
        stale_after_ms=stale_after_ms,
        non_rth_stale_after_ms=_positive_int(
            publisher_cfg.get("non_rth_stale_after_ms"),
            field_name="ibkr_reference_publisher.non_rth_stale_after_ms",
            default=stale_after_ms,
        ),
        reconnect_backoff_initial_ms=reconnect_backoff_initial_ms,
        reconnect_backoff_max_ms=reconnect_backoff_max_ms,
        dockerized_gateway=dict(scope.dockerized_gateway) if scope.dockerized_gateway else None,
        instruments=tuple(instruments),
    )


def compute_next_backoff_ms(
    *,
    current_backoff_ms: int | None,
    initial_backoff_ms: int,
    max_backoff_ms: int,
) -> int:
    initial = max(1, int(initial_backoff_ms))
    maximum = max(initial, int(max_backoff_ms))
    if current_backoff_ms is None or current_backoff_ms <= 0:
        return initial
    return min(maximum, max(initial, int(current_backoff_ms) * 2))


def classify_ibkr_session(now_utc: datetime | None = None) -> str:
    now_utc = now_utc or datetime.now(timezone.utc)
    et_now = now_utc.astimezone(_ET)
    day_of_week = et_now.weekday()
    current_time = et_now.time()

    if day_of_week == 5:
        return "OVERNIGHT" if current_time < _OVERNIGHT_END else "CLOSED"
    if day_of_week == 6:
        return "OVERNIGHT" if current_time >= _OVERNIGHT_START else "CLOSED"

    if current_time >= _OVERNIGHT_START or current_time < _OVERNIGHT_END:
        return "OVERNIGHT"
    if _RTH_END <= current_time < _POST_END:
        return "POST"
    if _RTH_START <= current_time < _RTH_END:
        return "RTH"
    if _PRE_START <= current_time < _RTH_START:
        return "PRE"
    return "CLOSED"


def _ts_event_ms(md: Mapping[str, Any] | None) -> int | None:
    if not isinstance(md, Mapping):
        return None
    raw = md.get("ts_event_ms", md.get("ts_event"))
    return int(raw) if isinstance(raw, int | float) else None


def _is_fresh(md: Mapping[str, Any] | None, *, now_ms: int, stale_after_ms: int) -> bool:
    if not isinstance(md, Mapping):
        return False
    bid = md.get("bid")
    ask = md.get("ask")
    ts_event_ms = _ts_event_ms(md)
    if not isinstance(bid, int | float) or not isinstance(ask, int | float):
        return False
    if not math.isfinite(float(bid)) or not math.isfinite(float(ask)):
        return False
    if bid <= 0 or ask <= 0 or ts_event_ms is None:
        return False
    return (now_ms - ts_event_ms) <= stale_after_ms


def select_reference_feed(
    *,
    session: str,
    smart_md: Mapping[str, Any] | None,
    overnight_md: Mapping[str, Any] | None,
    now_ms: int,
    stale_after_ms: int,
) -> tuple[str | None, Mapping[str, Any] | None]:
    smart_fresh = _is_fresh(smart_md, now_ms=now_ms, stale_after_ms=stale_after_ms)
    overnight_fresh = _is_fresh(overnight_md, now_ms=now_ms, stale_after_ms=stale_after_ms)
    session_name = session.strip().upper()

    if session_name == "CLOSED":
        return None, None

    if session_name in {"PRE", "RTH", "POST"}:
        if smart_fresh:
            return "SMART", smart_md
        if overnight_fresh:
            return "OVERNIGHT", overnight_md
    elif session_name == "OVERNIGHT":
        if overnight_fresh:
            return "OVERNIGHT", overnight_md
        if smart_fresh:
            return "SMART", smart_md

    if smart_fresh:
        return "SMART", smart_md
    if overnight_fresh:
        return "OVERNIGHT", overnight_md
    return None, None


class _RawIbkrReferenceClient(InteractiveBrokersClient):
    def __init__(
        self,
        *,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        host: str,
        port: int,
        client_id: int,
        request_timeout_secs: int,
    ) -> None:
        super().__init__(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            host=host,
            port=port,
            client_id=client_id,
            request_timeout_secs=request_timeout_secs,
        )
        self._reference_snapshots: dict[str, dict[str, dict[str, Any]]] = {}
        self._reference_snapshot_lock = threading.Lock()

    async def subscribe_reference_market_data(
        self,
        *,
        instrument_id: str,
        route: str,
        contract: IBContract,
    ) -> None:
        name = (instrument_id, "reference_market_data", route.upper())
        await self._subscribe(
            name,
            self._eclient.reqMktData,
            self._eclient.cancelMktData,
            contract,
            "",
            False,
            False,
            [],
        )

    async def unsubscribe_reference_market_data(self, *, instrument_id: str, route: str) -> None:
        name = (instrument_id, "reference_market_data", route.upper())
        await self._unsubscribe(name, self._eclient.cancelMktData)

    def snapshot_map(self) -> dict[str, dict[str, dict[str, Any]]]:
        with self._reference_snapshot_lock:
            return {
                instrument_id: {
                    route: dict(snapshot)
                    for route, snapshot in route_map.items()
                }
                for instrument_id, route_map in self._reference_snapshots.items()
            }

    def _update_reference_snapshot(
        self,
        *,
        req_id: int,
        tick_type: int,
        value: float,
    ) -> bool:
        subscription = self._subscriptions.get(req_id=req_id)
        if subscription is None:
            return False
        name = subscription.name
        if not (
            isinstance(name, tuple)
            and len(name) == 3
            and name[1] == "reference_market_data"
        ):
            return False

        instrument_id = str(name[0])
        route = str(name[2]).upper()
        update_map: dict[int, str] = {
            TickTypeEnum.BID: "bid",
            TickTypeEnum.ASK: "ask",
            TickTypeEnum.BID_SIZE: "bid_size",
            TickTypeEnum.ASK_SIZE: "ask_size",
        }
        field_name = update_map.get(int(tick_type))
        if field_name is None:
            return True
        if field_name in {"bid", "ask"} and value <= 0:
            return True
        if not math.isfinite(value):
            return True
        if field_name in {"bid_size", "ask_size"} and value < 0:
            return True

        with self._reference_snapshot_lock:
            snapshot = self._reference_snapshots.setdefault(instrument_id, {}).setdefault(route, {})
            snapshot[field_name] = float(value)
            snapshot["ts_event_ms"] = int(self._clock.timestamp_ns() // 1_000_000)
        return True

    async def process_tick_price(
        self,
        *,
        req_id: int,
        tick_type: int,
        price: float,
        attrib: Any,
    ) -> None:
        if self._update_reference_snapshot(req_id=req_id, tick_type=tick_type, value=price):
            return
        await super().process_tick_price(req_id=req_id, tick_type=tick_type, price=price, attrib=attrib)

    async def process_tick_size(
        self,
        *,
        req_id: int,
        tick_type: int,
        size: Any,
    ) -> None:
        if self._update_reference_snapshot(req_id=req_id, tick_type=tick_type, value=float(size)):
            return
        await super().process_tick_size(req_id=req_id, tick_type=tick_type, size=size)


class _ThreadedIbkrReferenceRuntime:
    def __init__(self, *, config: IbkrReferencePublisherConfig, logger: logging.Logger) -> None:
        self._config = config
        self._log = logger
        self._loop: asyncio.AbstractEventLoop | None = None
        self._client: _RawIbkrReferenceClient | None = None
        self._thread: threading.Thread | None = None
        self._client_port: int | None = None

    def connect(self) -> None:
        last_exc: Exception | None = None
        for port in self._candidate_ports():
            try:
                self._start_runtime(port=port)
                self._subscribe_all()
                self._client_port = port
                return
            except Exception as exc:
                last_exc = exc
                self.close()
        assert last_exc is not None
        raise last_exc

    def snapshot_map(self) -> dict[str, dict[str, dict[str, Any]]]:
        client = self._require_client()
        return client.snapshot_map()

    def close(self) -> None:
        client = self._client
        loop = self._loop
        thread = self._thread
        self._client = None
        self._loop = None
        self._thread = None
        if client is not None and loop is not None and not loop.is_closed():
            with suppress(Exception):
                stop_future = asyncio.run_coroutine_threadsafe(client._stop_async(), loop)
                stop_future.result(timeout=5)
        if loop is not None and not loop.is_closed():
            with suppress(Exception):
                loop.call_soon_threadsafe(loop.stop)
        if thread is not None and thread.is_alive():
            thread.join(timeout=5)

    def _candidate_ports(self) -> tuple[int, ...]:
        ports: list[int] = []
        if self._config.ibg_port is not None:
            ports.append(self._config.ibg_port)
        for port in self._config.ibg_fallback_ports:
            if port not in ports:
                ports.append(port)
        if not ports:
            raise ValueError("IBKR reference publisher requires `ibg_port` in the reference account scope")
        return tuple(ports)

    def _start_runtime(self, *, port: int) -> None:
        ready = threading.Event()
        holder: dict[str, Any] = {}

        def _run_loop() -> None:
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)
            clock = LiveClock()
            msgbus = MessageBus(
                trader_id=TraderId("EQUITIES-IBKR-REFERENCE-PUBLISHER"),
                clock=clock,
            )
            client = _RawIbkrReferenceClient(
                loop=loop,
                msgbus=msgbus,
                cache=Cache(database=None),
                clock=clock,
                host=self._config.ibg_host,
                port=port,
                client_id=self._config.ibg_client_id,
                request_timeout_secs=self._config.request_timeout_secs,
            )
            holder["loop"] = loop
            holder["client"] = client
            ready.set()
            loop.run_forever()
            pending = [task for task in asyncio.all_tasks(loop) if not task.done()]
            for task in pending:
                task.cancel()
            if pending:
                loop.run_until_complete(asyncio.gather(*pending, return_exceptions=True))
            loop.close()

        thread = threading.Thread(
            target=_run_loop,
            name=f"ibkr-reference-publisher-{self._config.ibg_client_id}-{port}",
            daemon=True,
        )
        thread.start()
        if not ready.wait(timeout=5):
            raise RuntimeError("Timed out starting IBKR reference publisher runtime")

        loop = holder["loop"]
        client = holder["client"]
        assert isinstance(loop, asyncio.AbstractEventLoop)
        assert isinstance(client, _RawIbkrReferenceClient)
        self._loop = loop
        self._client = client
        self._thread = thread

        loop.call_soon_threadsafe(client.start)
        wait_future = asyncio.run_coroutine_threadsafe(
            client.wait_until_ready(self._config.connection_timeout_secs),
            loop,
        )
        wait_future.result(timeout=self._config.connection_timeout_secs + 5)

    def _subscribe_all(self) -> None:
        loop = self._require_loop()
        future = asyncio.run_coroutine_threadsafe(self._subscribe_all_async(), loop)
        timeout = (
            self._config.connection_timeout_secs
            + (len(self._config.instruments) * self._config.request_timeout_secs)
        )
        future.result(timeout=timeout)

    async def _subscribe_all_async(self) -> None:
        client = self._require_client()
        for instrument in self._config.instruments:
            primary_exchange = instrument.primary_exchange
            smart_contract = IBContract(
                secType="STK",
                symbol=instrument.symbol,
                exchange="SMART",
                primaryExchange=primary_exchange,
                currency="USD",
            )
            details = await client.get_contract_details(smart_contract)
            if details:
                contract = getattr(details[0], "contract", None) or details[0]
                discovered_exchange = _optional_text(
                    getattr(contract, "primaryExchange", None) or getattr(contract, "exchange", None),
                )
                if discovered_exchange:
                    primary_exchange = discovered_exchange.upper()

            smart_contract = IBContract(
                secType="STK",
                symbol=instrument.symbol,
                exchange="SMART",
                primaryExchange=primary_exchange,
                currency="USD",
            )
            overnight_contract = IBContract(
                secType="STK",
                symbol=instrument.symbol,
                exchange="OVERNIGHT",
                primaryExchange=primary_exchange,
                currency="USD",
            )
            await client.subscribe_reference_market_data(
                instrument_id=instrument.instrument_id,
                route="SMART",
                contract=smart_contract,
            )
            await client.subscribe_reference_market_data(
                instrument_id=instrument.instrument_id,
                route="OVERNIGHT",
                contract=overnight_contract,
            )

    def _require_loop(self) -> asyncio.AbstractEventLoop:
        if self._loop is None:
            raise RuntimeError("IBKR reference publisher loop is not running")
        return self._loop

    def _require_client(self) -> _RawIbkrReferenceClient:
        if self._client is None:
            raise RuntimeError("IBKR reference publisher client is not running")
        return self._client


class IbkrReferencePublisherService:
    def __init__(
        self,
        *,
        config: IbkrReferencePublisherConfig,
        redis_client: Any,
        runtime_factory: type[_ThreadedIbkrReferenceRuntime] = _ThreadedIbkrReferenceRuntime,
        logger: logging.Logger | None = None,
        sleep_fn=time.sleep,
        time_ms_fn=None,
    ) -> None:
        self._config = config
        self._redis = redis_client
        self._runtime_factory = runtime_factory
        self._log = logger or _LOG
        self._sleep = sleep_fn
        self._time_ms = time_ms_fn or (lambda: int(time.time() * 1000))
        self._stop = False
        self._last_success_ts_ms: int | None = None
        self._last_error_type: str | None = None
        self._last_error_message: str | None = None
        self._current_backoff_ms: int | None = None
        self._runtime: _ThreadedIbkrReferenceRuntime | None = None

    def stop(self) -> None:
        self._stop = True
        if self._runtime is not None:
            with suppress(Exception):
                self._runtime.close()
            self._runtime = None

    def _stale_after_ms_for_session(self, session: str) -> int:
        normalized = str(session).strip().upper()
        if normalized == "RTH":
            return self._config.stale_after_ms
        return self._config.non_rth_stale_after_ms

    def publish_from_snapshot_map(
        self,
        snapshot_map: Mapping[str, Mapping[str, Mapping[str, Any]]],
        *,
        session: str,
        now_ms: int | None = None,
    ) -> dict[str, Any]:
        publish_ts_ms = self._time_ms() if now_ms is None else int(now_ms)
        stale_after_ms = self._stale_after_ms_for_session(session)
        instrument_status: dict[str, dict[str, Any]] = {}
        healthy_count = 0
        healthy_ts_values: list[int] = []

        for instrument in self._config.instruments:
            route_map = snapshot_map.get(instrument.instrument_id, {})
            smart_md = route_map.get("SMART")
            overnight_md = route_map.get("OVERNIGHT")
            route, selected_md = select_reference_feed(
                session=session,
                smart_md=smart_md,
                overnight_md=overnight_md,
                now_ms=publish_ts_ms,
                stale_after_ms=stale_after_ms,
            )

            if route is None or not isinstance(selected_md, Mapping):
                status_name = "missing"
                if smart_md or overnight_md:
                    status_name = "stale"
                instrument_status[instrument.instrument_id] = {
                    "state": status_name,
                    "route": None,
                    "age_ms": None,
                    "ts_event_ms": _ts_event_ms(smart_md) or _ts_event_ms(overnight_md),
                }
                continue

            ts_event_ms = _ts_event_ms(selected_md)
            age_ms = None if ts_event_ms is None else max(0, publish_ts_ms - ts_event_ms)
            quote_payload = {
                "profile_id": self._config.profile_id,
                "account_scope_id": self._config.account_scope_id,
                "service_id": self._config.service_id,
                "exchange": "ibkr",
                "instrument_id": instrument.instrument_id,
                "route": route,
                "session": session.upper(),
                "bid": float(selected_md["bid"]),
                "ask": float(selected_md["ask"]),
                "bid_size": float(selected_md.get("bid_size", 0.0) or 0.0),
                "ask_size": float(selected_md.get("ask_size", 0.0) or 0.0),
                "ts_event_ms": ts_event_ms,
                "ts_publish_ms": publish_ts_ms,
            }
            self._write_json(
                key=FluxRedisKeys.profile_market_last(
                    profile_id=self._config.profile_id,
                    account_scope_id=self._config.account_scope_id,
                    exchange="ibkr",
                    instrument_id=instrument.instrument_id,
                ),
                channel=FluxRedisKeys.profile_market_last_channel(
                    profile_id=self._config.profile_id,
                    account_scope_id=self._config.account_scope_id,
                    exchange="ibkr",
                    instrument_id=instrument.instrument_id,
                ),
                payload=quote_payload,
            )
            instrument_status[instrument.instrument_id] = {
                "state": "healthy",
                "route": route,
                "age_ms": age_ms,
                "ts_event_ms": ts_event_ms,
            }
            healthy_count += 1
            if ts_event_ms is not None:
                healthy_ts_values.append(ts_event_ms)

        if healthy_ts_values:
            self._last_success_ts_ms = max(healthy_ts_values)
            self._last_error_type = None
            self._last_error_message = None

        state = "stale"
        if healthy_count == len(self._config.instruments):
            state = "publishing"
        elif healthy_count > 0:
            state = "degraded"
        elif any(item["state"] == "missing" for item in instrument_status.values()):
            state = "degraded"

        return self._publish_status(
            state=state,
            connected=True,
            instrument_status=instrument_status,
            stale_after_ms=stale_after_ms,
            now_ms=publish_ts_ms,
        )

    def publish_connection_failure(
        self,
        exc: Exception,
        *,
        now_ms: int | None = None,
    ) -> dict[str, Any]:
        self._last_error_type = type(exc).__name__
        self._last_error_message = str(exc)
        return self._publish_status(
            state="down",
            connected=False,
            instrument_status={},
            stale_after_ms=self._config.stale_after_ms,
            now_ms=self._time_ms() if now_ms is None else int(now_ms),
        )

    def run_forever(self) -> None:
        if not self._config.enabled:
            self._publish_status(
                state="down",
                connected=False,
                instrument_status={},
                stale_after_ms=self._config.stale_after_ms,
                now_ms=self._time_ms(),
            )
            self._log.info("IBKR reference publisher disabled via config")
            return

        self._publish_status(
            state="starting",
            connected=False,
            instrument_status={},
            stale_after_ms=self._config.stale_after_ms,
            now_ms=self._time_ms(),
        )

        while not self._stop:
            runtime: _ThreadedIbkrReferenceRuntime | None = None
            try:
                runtime = self._runtime_factory(config=self._config, logger=self._log)
                runtime.connect()
                self._runtime = runtime
                self._current_backoff_ms = None
                self._publish_status(
                    state="connected",
                    connected=True,
                    instrument_status={},
                    stale_after_ms=self._config.stale_after_ms,
                    now_ms=self._time_ms(),
                )

                while not self._stop:
                    self.publish_from_snapshot_map(
                        runtime.snapshot_map(),
                        session=classify_ibkr_session(),
                        now_ms=self._time_ms(),
                    )
                    self._sleep(self._config.snapshot_interval_ms / 1_000.0)
            except KeyboardInterrupt:
                self.stop()
                raise
            except Exception as exc:
                self.publish_connection_failure(exc, now_ms=self._time_ms())
                self._current_backoff_ms = compute_next_backoff_ms(
                    current_backoff_ms=self._current_backoff_ms,
                    initial_backoff_ms=self._config.reconnect_backoff_initial_ms,
                    max_backoff_ms=self._config.reconnect_backoff_max_ms,
                )
                self._log.warning(
                    "IBKR reference publisher failure (%s): %s; retrying in %sms",
                    type(exc).__name__,
                    exc,
                    self._current_backoff_ms,
                )
                if self._stop:
                    break
                self._sleep(self._current_backoff_ms / 1_000.0)
            finally:
                if runtime is not None:
                    with suppress(Exception):
                        runtime.close()
                self._runtime = None

    def _publish_status(
        self,
        *,
        state: str,
        connected: bool,
        instrument_status: Mapping[str, Any],
        stale_after_ms: int,
        now_ms: int,
    ) -> dict[str, Any]:
        payload = {
            "profile_id": self._config.profile_id,
            "account_scope_id": self._config.account_scope_id,
            "service_id": self._config.service_id,
            "state": state,
            "connected": connected,
            "instrument_count": len(self._config.instruments),
            "instrument_status": dict(instrument_status),
            "last_success_ts_ms": self._last_success_ts_ms,
            "last_error_type": self._last_error_type,
            "last_error_message": self._last_error_message,
            "stale_after_ms": stale_after_ms,
            "ts_ms": now_ms,
        }
        self._write_json(
            key=FluxRedisKeys.profile_market_data_status(
                profile_id=self._config.profile_id,
                account_scope_id=self._config.account_scope_id,
                service_id=self._config.service_id,
            ),
            channel=FluxRedisKeys.profile_market_data_status_channel(
                profile_id=self._config.profile_id,
                account_scope_id=self._config.account_scope_id,
                service_id=self._config.service_id,
            ),
            payload=payload,
        )
        return payload

    def _write_json(self, *, key: str, channel: str, payload: Mapping[str, Any]) -> None:
        message = json.dumps(dict(payload), sort_keys=True)
        self._redis.set(key, message)
        self._redis.publish(channel, message)


__all__ = (
    "IbkrReferenceInstrument",
    "IbkrReferencePublisherConfig",
    "IbkrReferencePublisherService",
    "build_ibkr_reference_publisher_config",
    "classify_ibkr_session",
    "compute_next_backoff_ms",
    "select_reference_feed",
)
