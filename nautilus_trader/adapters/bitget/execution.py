# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
import json
import re
import time
from datetime import timedelta
from decimal import Decimal
from types import SimpleNamespace
from typing import Any

from nautilus_trader.adapters.bitget.config import BitgetExecClientConfig
from nautilus_trader.adapters.bitget.constants import BITGET_DEFAULT_PRODUCTS
from nautilus_trader.adapters.bitget.constants import BITGET_VENUE
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BitgetExecutionClient(LiveExecutionClient):
    """Minimal Bitget execution client scaffold."""

    @staticmethod
    def _default_account_id(client_name: str) -> AccountId:
        return AccountId(f"{client_name}-001")

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: Any,
        msgbus: MessageBus,
        cache: Any,
        clock: LiveClock,
        instrument_provider: InstrumentProvider,
        config: BitgetExecClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or "BITGET"),
            venue=BITGET_VENUE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.CASH,
            base_currency=None,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )
        self._http_client = client
        self._config = config
        self._environment = (
            nautilus_pyo3.BitgetEnvironment.DEMO
            if config.demo
            else nautilus_pyo3.BitgetEnvironment.MAINNET
        )
        self._set_account_id(BitgetExecutionClient._default_account_id(name or BITGET_VENUE.value))
        self._product_types = tuple(config.product_types) if config.product_types else tuple(
            BITGET_DEFAULT_PRODUCTS,
        )
        self._ws_client: Any | None = None
        self._ws_tasks: set[asyncio.Task] = set()

    async def _connect(self) -> None:
        if (
            not self._config.api_key
            or not self._config.api_secret
            or not self._config.api_passphrase
        ):
            self._log.warning(
                "Bitget execution client missing private WebSocket credentials; skipping connect",
            )
            return

        ws_url = self._config.base_url_ws_private or nautilus_pyo3.get_bitget_ws_private_url(
            self._environment,
        )
        ws_config = nautilus_pyo3.WebSocketConfig(
            url=ws_url,
            headers=[],
            heartbeat=30,
            heartbeat_msg=nautilus_pyo3.BitgetWebSocketClient.ping_message(),
            reconnect_timeout_ms=10_000,
            reconnect_delay_initial_ms=self._config.retry_delay_initial_ms or 2_000,
            reconnect_delay_max_ms=self._config.retry_delay_max_ms or 30_000,
        )

        self._ws_client = await nautilus_pyo3.WebSocketClient.connect(
            loop_=self._loop,
            config=ws_config,
            handler=self._handle_ws_message,
            post_reconnection=self._handle_ws_reconnect,
        )
        await self._authenticate_ws()
        self._log.info(f"Bitget execution client connected to {ws_url}")

    async def _disconnect(self) -> None:
        self._log.info("Bitget execution client disconnected")

    def _handle_ws_message(self, raw: bytes) -> None:
        try:
            message = raw.decode("utf-8")
            if not message or message == "pong":
                return

            payload = json.loads(message)
            if not isinstance(payload, dict):
                self._log.debug(f"Bitget private WebSocket message: {message}")
                return

            event = payload.get("event")
            raw_code = payload.get("code")
            code = "" if raw_code is None else str(raw_code)
            raw_msg = payload.get("msg")
            msg = "" if raw_msg is None else str(raw_msg)
            arg = payload.get("arg") or {}

            if event == "login" and code == "0":
                self._loop.call_soon_threadsafe(self._on_ws_authenticated)
                return

            if event == "subscribe":
                channel = str(arg.get("channel") or "")
                inst_type = str(arg.get("instType") or "")
                self._log.info(
                    f"Bitget private WebSocket subscribed: channel={channel} instType={inst_type}",
                )
                return

            if event == "error":
                if "login" in msg.lower() or code == "30005":
                    self._log.warning(
                        f"Bitget private WebSocket login failed: code={code} msg={msg}",
                    )
                else:
                    self._log.warning(
                        f"Bitget private WebSocket error: code={code} msg={msg}",
                    )
                return

            channel = str(arg.get("channel") or "")
            if channel == "account":
                self._handle_account_channel(payload)
                return
            if channel == "orders":
                self._handle_orders_channel(payload)
                return
            if channel == "fill":
                self._handle_fill_channel(payload)
                return
            if channel == "positions":
                self._handle_positions_channel(payload)
                return

            self._log.debug(f"Bitget private WebSocket message: {message}")
        except Exception as e:
            self._log.error(f"Error handling Bitget private WebSocket message: {e}")

    def _handle_ws_reconnect(self) -> None:
        self._loop.call_soon_threadsafe(self._on_ws_reconnect)

    def _on_ws_authenticated(self) -> None:
        self._log.info("Bitget private WebSocket authenticated")
        task = self.create_task(
            self._subscribe_private_ws(),
            log_msg="bitget:subscribe_private_ws",
        )
        if task:
            self._ws_tasks.add(task)

    def _on_ws_reconnect(self) -> None:
        self._log.warning("Bitget private WebSocket reconnected; re-authenticating")
        task = self.create_task(
            self._authenticate_ws(),
            log_msg="bitget:reauth_private_ws",
        )
        if task:
            self._ws_tasks.add(task)

    def _handle_account_channel(self, payload: dict[str, Any]) -> None:
        data = payload.get("data") or []
        if not data:
            self._log.debug("Bitget private account payload received: 0 entries")
            return

        balances: list[AccountBalance] = []
        latest_update_ms = 0

        for entry in data:
            currency_code = str(entry.get("coin") or entry.get("marginCoin") or "").strip()
            if not currency_code:
                self._log.debug(f"Skipping Bitget account payload entry without currency: {entry}")
                continue

            currency = Currency.from_str(currency_code)
            free_amount = Decimal(str(entry.get("available") or "0"))
            frozen = Decimal(str(entry.get("frozen") or "0"))
            locked_extra = Decimal(str(entry.get("locked") or "0"))
            locked_amount = frozen + locked_extra
            total_amount = Decimal(str(entry.get("equity") or (free_amount + locked_amount)))
            free = Money(free_amount, currency)
            locked = Money(locked_amount, currency)
            total = Money(total_amount, currency)
            balances.append(
                AccountBalance(
                    total=total,
                    locked=locked,
                    free=free,
                ),
            )
            latest_update_ms = max(latest_update_ms, int(entry.get("uTime") or 0))

        if not balances:
            self._log.debug("Bitget private account payload produced no balances")
            return

        self.generate_account_state(
            balances=balances,
            margins=[],
            reported=True,
            ts_event=millis_to_nanos(latest_update_ms),
        )

    @staticmethod
    def _normalize_private_order_status(status: Any) -> str:
        normalized = str(status or "").strip().lower().replace("_", "-")
        if normalized in {"partially-filled", "partial-fill"}:
            return "partial-fill"
        if normalized in {"filled", "full-fill"}:
            return "full-fill"
        if normalized in {"cancelled", "canceled"}:
            return "cancelled"
        return normalized

    @staticmethod
    def _parse_private_liquidity_side(trade_scope: Any) -> LiquiditySide:
        normalized = str(trade_scope or "").strip().lower()
        if normalized in {"maker", "m", "marker"}:
            return LiquiditySide.MAKER
        if normalized in {"taker", "t"}:
            return LiquiditySide.TAKER
        return LiquiditySide.NO_LIQUIDITY_SIDE

    @staticmethod
    def _product_type_key(product_type: Any) -> str:
        normalized = str(product_type or "").strip().upper().replace("_", "-")
        if normalized.endswith("SPOT"):
            return "SPOT"
        if normalized.endswith("USDT-FUTURES"):
            return "USDT-FUTURES"
        if normalized.endswith("COIN-FUTURES"):
            return "COIN-FUTURES"
        if normalized.endswith("USDC-FUTURES"):
            return "USDC-FUTURES"
        return normalized

    def _product_type_for_instrument(self, instrument: Any) -> Any:
        settlement_code = BitgetExecutionClient._currency_code(
            getattr(instrument, "settlement_currency", None),
        )
        quote_code = BitgetExecutionClient._currency_code(
            getattr(instrument, "quote_currency", None),
        )
        base_code = BitgetExecutionClient._currency_code(
            getattr(instrument, "base_currency", None) or getattr(instrument, "underlying", None),
        )
        if settlement_code:
            if settlement_code == base_code:
                return nautilus_pyo3.BitgetProductType.COIN_FUTURES
            if settlement_code == "USDC" or quote_code == "USDC":
                return nautilus_pyo3.BitgetProductType.USDC_FUTURES
            if settlement_code == "USDT" or quote_code == "USDT":
                return nautilus_pyo3.BitgetProductType.USDT_FUTURES

        instrument_id = getattr(instrument, "id", None)
        symbol = getattr(getattr(instrument_id, "symbol", None), "value", None)
        if symbol is None:
            symbol = str(instrument_id)
        return BitgetExecutionClient._infer_product_type_from_symbol(symbol)

    @staticmethod
    def _field(payload: Any, key: str, default: Any = None) -> Any:
        if isinstance(payload, dict):
            return payload.get(key, default)
        return getattr(payload, key, default)

    @staticmethod
    def _parse_response_payload(payload: Any) -> Any:
        if payload is None:
            return None
        if isinstance(payload, str):
            return json.loads(payload)
        return payload

    @staticmethod
    def _payload_items(payload: Any) -> list[Any]:
        payload = BitgetExecutionClient._parse_response_payload(payload)
        if payload is None:
            return []
        if isinstance(payload, list):
            return payload
        return [payload]

    @staticmethod
    def _string_value(value: Any) -> str:
        if value is None:
            return ""
        return str(value).strip()

    @staticmethod
    def _account_mode_from_config(config: Any) -> str | None:
        value = BitgetExecutionClient._string_value(getattr(config, "account_mode", None))
        return value.upper() or None

    @staticmethod
    def _margin_mode_from_config(config: Any) -> str | None:
        value = BitgetExecutionClient._string_value(getattr(config, "margin_mode", None))
        return value.lower() or None

    @staticmethod
    def _position_mode_from_config(config: Any) -> str | None:
        value = BitgetExecutionClient._string_value(getattr(config, "position_mode", None))
        return value.lower().replace("-", "_").replace(" ", "_") or None

    @staticmethod
    def _allow_cash_borrowing_from_config(config: Any) -> bool:
        return bool(getattr(config, "allow_cash_borrowing", False))

    @staticmethod
    def _format_exchange_error_reason(error: Exception) -> str:
        reason = str(error).strip()
        if not reason:
            return reason
        if reason.startswith("bitget_http_error:"):
            return reason

        match = re.match(r"^HTTP request failed with status (\d+)(?: body=(.+))?$", reason)
        if not match:
            return reason

        status = match.group(1)
        body = match.group(2)
        if not body:
            return f"bitget_http_error: status={status}"

        try:
            payload = BitgetExecutionClient._parse_response_payload(body)
        except Exception:
            return f"bitget_http_error: status={status}"

        if isinstance(payload, dict):
            code = BitgetExecutionClient._string_value(BitgetExecutionClient._field(payload, "code"))
            msg = BitgetExecutionClient._string_value(BitgetExecutionClient._field(payload, "msg"))
            parts = [f"bitget_http_error: status={status}"]
            if code:
                parts.append(f"code={code}")
            if msg:
                parts.append(f"msg={msg}")
            return " ".join(parts)

        return f"bitget_http_error: status={status}"

    @staticmethod
    def _is_delivery_symbol(symbol: str) -> bool:
        if "-" not in symbol:
            return False
        suffix = symbol.rsplit("-", 1)[1]
        return len(suffix) == 6 and suffix.isdigit()

    @staticmethod
    def _raw_symbol_from_instrument_id(instrument_id: Any) -> str:
        symbol = getattr(getattr(instrument_id, "symbol", None), "value", None)
        if symbol is None:
            symbol = str(instrument_id)
        if symbol.endswith("-PERP"):
            return symbol[:-5]
        if BitgetExecutionClient._is_delivery_symbol(symbol):
            return symbol.rsplit("-", 1)[0]
        return symbol

    @staticmethod
    def _infer_product_type_from_symbol(symbol: str) -> Any:
        raw_symbol = symbol.split(".", 1)[0]
        if raw_symbol.endswith("-PERP"):
            raw_symbol = raw_symbol[:-5]
        elif BitgetExecutionClient._is_delivery_symbol(raw_symbol):
            raw_symbol = raw_symbol.rsplit("-", 1)[0]
        else:
            return nautilus_pyo3.BitgetProductType.SPOT

        if raw_symbol.endswith("USDC"):
            return nautilus_pyo3.BitgetProductType.USDC_FUTURES
        if raw_symbol.endswith("USDT"):
            return nautilus_pyo3.BitgetProductType.USDT_FUTURES
        if raw_symbol.endswith("USD"):
            return nautilus_pyo3.BitgetProductType.COIN_FUTURES
        return nautilus_pyo3.BitgetProductType.SPOT

    @staticmethod
    def _infer_margin_coin_from_raw_symbol(raw_symbol: str) -> str | None:
        for quote in ("USDC", "USDT", "USD"):
            if raw_symbol.endswith(quote):
                base = raw_symbol[: -len(quote)]
                if quote == "USD":
                    return base or None
                return quote
        return None

    def _product_type_for_instrument_id(self, instrument_id: Any) -> Any:
        instrument = BitgetExecutionClient._resolve_instrument(self, instrument_id)
        if instrument is not None:
            return BitgetExecutionClient._product_type_for_instrument(self, instrument)

        symbol = getattr(getattr(instrument_id, "symbol", None), "value", None)
        if symbol is None:
            symbol = str(instrument_id)
        return BitgetExecutionClient._infer_product_type_from_symbol(symbol)

    @staticmethod
    def _currency_code(currency: Any) -> str:
        if currency is None:
            return ""
        code = getattr(currency, "code", None)
        if code is None:
            return str(currency).strip().upper()
        if hasattr(code, "as_str"):
            return str(code.as_str()).strip().upper()
        return str(code).strip().upper()

    def _margin_coin_for_instrument_id(self, instrument_id: Any) -> str | None:
        product_type = BitgetExecutionClient._product_type_for_instrument_id(self, instrument_id)
        if BitgetExecutionClient._product_type_key(product_type) == "SPOT":
            return None

        instrument = BitgetExecutionClient._resolve_instrument(self, instrument_id)
        if instrument is not None:
            margin_coin = BitgetExecutionClient._currency_code(
                getattr(instrument, "settlement_currency", None),
            ) or BitgetExecutionClient._currency_code(getattr(instrument, "quote_currency", None))
            return margin_coin or None

        product_type_key = BitgetExecutionClient._product_type_key(product_type)
        if product_type_key == "USDT-FUTURES":
            return "USDT"
        if product_type_key == "USDC-FUTURES":
            return "USDC"
        if product_type_key == "COIN-FUTURES":
            return BitgetExecutionClient._infer_margin_coin_from_raw_symbol(
                BitgetExecutionClient._raw_symbol_from_instrument_id(instrument_id),
            )
        return None

    def _resolve_instrument(self, instrument_id: Any) -> Any | None:
        cache_get = getattr(self._cache, "instrument", None)
        if callable(cache_get):
            instrument = cache_get(instrument_id)
            if instrument is not None:
                return instrument

        provider_find = getattr(self._instrument_provider, "find", None)
        if callable(provider_find):
            return provider_find(instrument_id)

        return None

    def _resolve_instrument_by_symbol(
        self,
        product_type: Any,
        raw_symbol: str,
        fallback_instrument_id: Any | None = None,
    ) -> Any | None:
        if fallback_instrument_id is not None:
            instrument = BitgetExecutionClient._resolve_instrument(self, fallback_instrument_id)
            if instrument is not None:
                return instrument

        instrument_ids = getattr(self._cache, "instrument_ids", None)
        cache_get = getattr(self._cache, "instrument", None)
        if callable(instrument_ids) and callable(cache_get):
            for instrument_id in instrument_ids(venue=BITGET_VENUE):
                instrument = cache_get(instrument_id)
                instrument_raw_symbol = getattr(getattr(instrument, "raw_symbol", None), "value", None)
                if (
                    instrument is not None
                    and instrument_raw_symbol == raw_symbol
                    and BitgetExecutionClient._product_type_key(
                        BitgetExecutionClient._product_type_for_instrument(self, instrument),
                    )
                    == BitgetExecutionClient._product_type_key(product_type)
                ):
                    return instrument

        return None

    @staticmethod
    def _order_side_to_api_str(side: OrderSide) -> str:
        return "buy" if side == OrderSide.BUY else "sell"

    @staticmethod
    def _order_type_to_api_str(order_type: OrderType) -> str:
        if order_type == OrderType.MARKET:
            return "market"
        return "limit"

    @staticmethod
    def _time_in_force_to_api_force(order: Any) -> str | None:
        if getattr(order, "order_type", None) == OrderType.MARKET:
            return None
        if getattr(order, "is_post_only", False):
            return "post_only"

        tif = getattr(order, "time_in_force", None)
        if tif == TimeInForce.IOC:
            return "ioc"
        if tif == TimeInForce.FOK:
            return "fok"
        return "gtc"

    @staticmethod
    def _time_in_force_from_api_force(force: str, post_only: bool) -> TimeInForce:
        if post_only or force == "post_only":
            return TimeInForce.GTC
        if force == "ioc":
            return TimeInForce.IOC
        if force == "fok":
            return TimeInForce.FOK
        return TimeInForce.GTC

    @staticmethod
    def _parse_order_side(value: Any) -> OrderSide:
        return OrderSide.BUY if str(value or "").strip().lower() == "buy" else OrderSide.SELL

    @staticmethod
    def _parse_order_type(value: Any) -> OrderType:
        return OrderType.MARKET if str(value or "").strip().lower() == "market" else OrderType.LIMIT

    @staticmethod
    def _parse_order_status(payload: Any) -> OrderStatus:
        status = BitgetExecutionClient._normalize_private_order_status(
            BitgetExecutionClient._field(payload, "status")
            or BitgetExecutionClient._field(payload, "orderStatus"),
        )
        filled_qty = Decimal(
            BitgetExecutionClient._string_value(
                BitgetExecutionClient._field(payload, "baseVolume")
                or BitgetExecutionClient._field(payload, "cumExecQty")
                or BitgetExecutionClient._field(payload, "filledQty")
                or "0",
            ),
        )

        if status in {"partial-fill", "partially-filled"} or (
            status in {"new", "live"} and filled_qty > 0
        ):
            return OrderStatus.PARTIALLY_FILLED
        if status in {"full-fill", "filled"}:
            return OrderStatus.FILLED
        if status == "cancelled":
            return OrderStatus.CANCELED
        if status == "expired":
            return OrderStatus.EXPIRED
        if status in {"rejected", "fail", "failed"}:
            return OrderStatus.REJECTED
        return OrderStatus.ACCEPTED

    @staticmethod
    def _timestamp_ns_from_value(value: Any, fallback: int = 0) -> int:
        text = BitgetExecutionClient._string_value(value)
        if not text:
            return fallback
        try:
            return millis_to_nanos(int(text))
        except ValueError:
            return fallback

    @staticmethod
    def _datetime_to_millis(value: Any) -> int | None:
        if value is None:
            return None
        return int(value.timestamp() * 1000)

    @staticmethod
    def _is_truthy_flag(value: Any) -> bool:
        return str(value or "").strip().upper() in {"YES", "TRUE", "1"}

    def _build_order_status_report(
        self,
        payload: Any,
        product_type: Any,
        fallback_instrument_id: Any | None = None,
        fallback_client_order_id: ClientOrderId | None = None,
        fallback_venue_order_id: VenueOrderId | None = None,
        ts_init: int | None = None,
    ) -> OrderStatusReport | None:
        raw_symbol = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "symbol"),
        ) or (
            BitgetExecutionClient._raw_symbol_from_instrument_id(fallback_instrument_id)
            if fallback_instrument_id is not None
            else ""
        )
        instrument = BitgetExecutionClient._resolve_instrument_by_symbol(
            self,
            product_type,
            raw_symbol,
            fallback_instrument_id=fallback_instrument_id,
        )
        if instrument is None:
            return None

        order_id = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "orderId"),
        ) or (fallback_venue_order_id.value if fallback_venue_order_id else "")
        if not order_id:
            return None

        client_order_id = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "clientOid"),
        )
        report_client_order_id = (
            ClientOrderId(client_order_id) if client_order_id else fallback_client_order_id
        )

        status = BitgetExecutionClient._parse_order_status(payload)
        quantity_raw = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "size")
            or BitgetExecutionClient._field(payload, "qty"),
        ) or "0"
        filled_qty_raw = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "baseVolume")
            or BitgetExecutionClient._field(payload, "cumExecQty")
            or BitgetExecutionClient._field(payload, "filledQty")
            or "0",
        )
        if status == OrderStatus.FILLED and filled_qty_raw in {"", "0"}:
            filled_qty_raw = quantity_raw

        force = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "force")
            or BitgetExecutionClient._field(payload, "timeInForce"),
        ).lower()
        post_only = force == "post_only"
        price_raw = BitgetExecutionClient._string_value(BitgetExecutionClient._field(payload, "price"))
        avg_px_raw = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "priceAvg")
            or BitgetExecutionClient._field(payload, "avgPrice"),
        )
        accepted_ts = BitgetExecutionClient._timestamp_ns_from_value(
            BitgetExecutionClient._field(payload, "cTime")
            or BitgetExecutionClient._field(payload, "createdTime")
            or BitgetExecutionClient._field(payload, "ctime"),
        )
        last_ts = BitgetExecutionClient._timestamp_ns_from_value(
            BitgetExecutionClient._field(payload, "uTime")
            or BitgetExecutionClient._field(payload, "updatedTime")
            or BitgetExecutionClient._field(payload, "utime")
            or BitgetExecutionClient._field(payload, "cTime"),
            fallback=accepted_ts,
        )

        return OrderStatusReport(
            account_id=self.account_id,
            instrument_id=instrument.id,
            client_order_id=report_client_order_id,
            venue_order_id=VenueOrderId(order_id),
            order_side=BitgetExecutionClient._parse_order_side(
                BitgetExecutionClient._field(payload, "side"),
            ),
            order_type=BitgetExecutionClient._parse_order_type(
                BitgetExecutionClient._field(payload, "orderType"),
            ),
            time_in_force=BitgetExecutionClient._time_in_force_from_api_force(force, post_only),
            order_status=status,
            price=Price.from_str(price_raw) if price_raw and price_raw != "0" else None,
            quantity=Quantity.from_str(quantity_raw),
            filled_qty=Quantity.from_str(filled_qty_raw or "0"),
            avg_px=Decimal(avg_px_raw) if avg_px_raw and avg_px_raw != "0" else None,
            post_only=post_only,
            reduce_only=BitgetExecutionClient._is_truthy_flag(
                BitgetExecutionClient._field(payload, "reduceOnly"),
            ),
            ts_accepted=accepted_ts or (ts_init or self._clock.timestamp_ns()),
            ts_last=last_ts or (ts_init or self._clock.timestamp_ns()),
            report_id=UUID4(),
            ts_init=ts_init or self._clock.timestamp_ns(),
        )

    def _build_fill_report(
        self,
        payload: Any,
        product_type: Any,
        fallback_instrument_id: Any | None = None,
        ts_init: int | None = None,
    ) -> FillReport | None:
        raw_symbol = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "symbol"),
        ) or (
            BitgetExecutionClient._raw_symbol_from_instrument_id(fallback_instrument_id)
            if fallback_instrument_id is not None
            else ""
        )
        instrument = BitgetExecutionClient._resolve_instrument_by_symbol(
            self,
            product_type,
            raw_symbol,
            fallback_instrument_id=fallback_instrument_id,
        )
        if instrument is None:
            return None

        order_id = BitgetExecutionClient._string_value(BitgetExecutionClient._field(payload, "orderId"))
        trade_id = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "tradeId")
            or BitgetExecutionClient._field(payload, "execId"),
        )
        if not order_id or not trade_id:
            return None

        fee_detail = BitgetExecutionClient._field(payload, "feeDetail") or {}
        if isinstance(fee_detail, list):
            fee_detail = fee_detail[0] if fee_detail else {}

        fee_coin = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(fee_detail, "feeCoin")
            or BitgetExecutionClient._field(payload, "feeCoin"),
        )
        commission_currency = (
            Currency.from_str(fee_coin) if fee_coin else instrument.quote_currency
        )
        total_fee = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(fee_detail, "totalFee")
            or BitgetExecutionClient._field(fee_detail, "fee")
            or BitgetExecutionClient._field(payload, "fillFee")
            or "0",
        )
        last_px_raw = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "priceAvg")
            or BitgetExecutionClient._field(payload, "avgPrice")
            or BitgetExecutionClient._field(payload, "execPrice")
            or BitgetExecutionClient._field(payload, "price")
            or "0",
        )
        last_qty_raw = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "size")
            or BitgetExecutionClient._field(payload, "qty")
            or BitgetExecutionClient._field(payload, "execQty")
            or BitgetExecutionClient._field(payload, "fillQty")
            or "0",
        )

        return FillReport(
            account_id=self.account_id,
            instrument_id=instrument.id,
            venue_order_id=VenueOrderId(order_id),
            venue_position_id=None,
            trade_id=TradeId(trade_id),
            order_side=BitgetExecutionClient._parse_order_side(
                BitgetExecutionClient._field(payload, "side"),
            ),
            last_qty=Quantity.from_str(last_qty_raw),
            last_px=Price.from_str(last_px_raw),
            commission=Money(Decimal(total_fee or "0"), commission_currency),
            liquidity_side=BitgetExecutionClient._parse_private_liquidity_side(
                BitgetExecutionClient._field(payload, "tradeScope"),
            ),
            ts_event=BitgetExecutionClient._timestamp_ns_from_value(
                BitgetExecutionClient._field(payload, "uTime")
                or BitgetExecutionClient._field(payload, "updatedTime")
                or BitgetExecutionClient._field(payload, "createdTime")
                or BitgetExecutionClient._field(payload, "cTime"),
            )
            or (ts_init or self._clock.timestamp_ns()),
            report_id=UUID4(),
            ts_init=ts_init or self._clock.timestamp_ns(),
        )

    def _build_position_status_report(
        self,
        payload: Any,
        product_type: Any,
        fallback_instrument_id: Any | None = None,
        ts_init: int | None = None,
    ) -> PositionStatusReport | None:
        raw_symbol = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "symbol"),
        ) or (
            BitgetExecutionClient._raw_symbol_from_instrument_id(fallback_instrument_id)
            if fallback_instrument_id is not None
            else ""
        )
        instrument = BitgetExecutionClient._resolve_instrument_by_symbol(
            self,
            product_type,
            raw_symbol,
            fallback_instrument_id=fallback_instrument_id,
        )
        if instrument is None:
            return None

        total = Decimal(
            BitgetExecutionClient._string_value(BitgetExecutionClient._field(payload, "total") or "0"),
        )
        ts_event = BitgetExecutionClient._timestamp_ns_from_value(
            BitgetExecutionClient._field(payload, "uTime")
            or BitgetExecutionClient._field(payload, "updatedTime"),
        ) or (ts_init or self._clock.timestamp_ns())
        venue_position_id = None
        pos_id = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "posId")
            or BitgetExecutionClient._field(payload, "positionId"),
        )
        if pos_id:
            venue_position_id = PositionId(pos_id)

        avg_px_raw = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "openPriceAvg")
            or BitgetExecutionClient._field(payload, "avgPrice"),
        )
        avg_px_open = Decimal(avg_px_raw) if avg_px_raw and avg_px_raw != "0" else None

        if total == 0:
            return PositionStatusReport(
                account_id=self.account_id,
                instrument_id=instrument.id,
                position_side=PositionSide.FLAT,
                quantity=Quantity.zero(instrument.size_precision),
                venue_position_id=venue_position_id,
                avg_px_open=None,
                report_id=UUID4(),
                ts_last=ts_event,
                ts_init=ts_init or self._clock.timestamp_ns(),
            )

        hold_side = BitgetExecutionClient._string_value(
            BitgetExecutionClient._field(payload, "holdSide")
            or BitgetExecutionClient._field(payload, "posSide"),
        ).lower()
        position_side = PositionSide.SHORT if hold_side == "short" else PositionSide.LONG
        try:
            quantity = instrument.make_qty(str(abs(total)), round_down=True)
        except TypeError:
            quantity = instrument.make_qty(str(abs(total)))
        except ValueError:
            quantity = Quantity.from_str(str(abs(total)))

        return PositionStatusReport(
            account_id=self.account_id,
            instrument_id=instrument.id,
            position_side=position_side,
            quantity=quantity,
            venue_position_id=venue_position_id,
            avg_px_open=avg_px_open,
            report_id=UUID4(),
            ts_last=ts_event,
            ts_init=ts_init or self._clock.timestamp_ns(),
        )

    @staticmethod
    def _check_order_validity(order: Any, product_type: Any) -> str | None:
        if getattr(order, "order_type", None) not in (OrderType.LIMIT, OrderType.MARKET):
            return "UNSUPPORTED_ORDER_TYPE"
        if getattr(order, "is_post_only", False) and getattr(order, "order_type", None) != OrderType.LIMIT:
            return "UNSUPPORTED_POST_ONLY"
        if getattr(order, "is_reduce_only", False) and BitgetExecutionClient._product_type_key(product_type) == "SPOT":
            return "UNSUPPORTED_REDUCE_ONLY_SPOT"
        return None

    def _handle_orders_channel(self, payload: dict[str, Any]) -> None:
        data = payload.get("data") or []
        if not data:
            self._log.debug("Bitget private orders payload received: 0 entries")
            return

        for entry in data:
            client_oid = str(entry.get("clientOid") or "").strip()
            order_id = str(entry.get("orderId") or "").strip()

            client_order_id = ClientOrderId(client_oid) if client_oid else None
            venue_order_id = VenueOrderId(order_id) if order_id else None

            if client_order_id is None and venue_order_id is not None:
                client_order_id = self._cache.client_order_id(venue_order_id)

            order = self._cache.order(client_order_id) if client_order_id is not None else None
            if order is None:
                lookup = client_oid or order_id or "<missing>"
                self._log.warning(
                    f"Bitget private order update ignored: order not found for {lookup}",
                )
                continue

            client_order_id = order.client_order_id
            venue_order_id = venue_order_id or self._cache.venue_order_id(client_order_id)
            if venue_order_id is None:
                self._log.warning(
                    "Bitget private order update ignored: "
                    f"venue order ID missing for {client_order_id!r}",
                )
                continue

            status = BitgetExecutionClient._normalize_private_order_status(entry.get("status"))
            ts_event = millis_to_nanos(int(entry.get("uTime") or payload.get("ts") or 0))

            quantity = Quantity.from_str(str(entry.get("size") or order.quantity))
            cached_price = order.price if getattr(order, "has_price", False) else None
            price_raw = str(entry.get("price") or "").strip()
            price = Price.from_str(price_raw) if price_raw and price_raw != "0" else cached_price
            trigger_price = getattr(order, "trigger_price", None)
            is_updated = quantity != order.quantity or price != cached_price

            if status in {"new", "live", "partial-fill"}:
                if order.status in (OrderStatus.CANCELED, OrderStatus.EXPIRED, OrderStatus.FILLED):
                    self._log.debug(
                        f"Bitget private order update ignored for terminal order "
                        f"{client_order_id!r}: status={status}",
                    )
                elif order.status == OrderStatus.ACCEPTED:
                    if is_updated:
                        self.generate_order_updated(
                            strategy_id=order.strategy_id,
                            instrument_id=order.instrument_id,
                            client_order_id=client_order_id,
                            venue_order_id=venue_order_id,
                            quantity=quantity,
                            price=price,
                            trigger_price=trigger_price,
                            ts_event=ts_event,
                        )
                else:
                    self.generate_order_accepted(
                        strategy_id=order.strategy_id,
                        instrument_id=order.instrument_id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        ts_event=ts_event,
                    )
                continue

            if status == "cancelled":
                if order.status != OrderStatus.CANCELED:
                    self.generate_order_canceled(
                        strategy_id=order.strategy_id,
                        instrument_id=order.instrument_id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        ts_event=ts_event,
                    )
                continue

            if status == "expired":
                if order.status != OrderStatus.EXPIRED:
                    self.generate_order_expired(
                        strategy_id=order.strategy_id,
                        instrument_id=order.instrument_id,
                        client_order_id=client_order_id,
                        venue_order_id=venue_order_id,
                        ts_event=ts_event,
                    )
                continue

            if status == "full-fill":
                self._log.debug(
                    f"Bitget private order update received full-fill for {client_order_id!r}; "
                    "awaiting fill channel",
                )
                continue

            self._log.debug(
                f"Bitget private order update received unhandled status {status!r} "
                f"for {client_order_id!r}",
            )

    def _handle_fill_channel(self, payload: dict[str, Any]) -> None:
        data = payload.get("data") or []
        if not data:
            self._log.debug("Bitget private fills payload received: 0 entries")
            return

        for entry in data:
            order_id = str(entry.get("orderId") or "").strip()
            trade_id_value = str(entry.get("tradeId") or "").strip()

            if not order_id or not trade_id_value:
                self._log.warning(
                    "Bitget private fill update ignored: missing orderId or tradeId",
                )
                continue

            venue_order_id = VenueOrderId(order_id)
            client_order_id = self._cache.client_order_id(venue_order_id)
            order = self._cache.order(client_order_id) if client_order_id is not None else None
            if order is None:
                self._log.warning(
                    f"Bitget private fill update ignored: order not found for {order_id}",
                )
                continue

            instrument = self._cache.instrument(order.instrument_id)
            if instrument is None:
                self._log.warning(
                    f"Bitget private fill update ignored: instrument not found for "
                    f"{order.instrument_id}",
                )
                continue

            fee_details = entry.get("feeDetail") or []
            fee_detail = fee_details[0] if fee_details else {}
            commission_currency = instrument.quote_currency
            fee_coin = str(fee_detail.get("feeCoin") or "").strip()
            if fee_coin:
                commission_currency = Currency.from_str(fee_coin)

            self.generate_order_filled(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=venue_order_id,
                venue_position_id=None,
                trade_id=TradeId(trade_id_value),
                order_side=order.side,
                order_type=order.order_type,
                last_qty=Quantity.from_str(str(entry.get("size") or "0")),
                last_px=Price.from_str(str(entry.get("priceAvg") or entry.get("price") or "0")),
                quote_currency=instrument.quote_currency,
                commission=Money(
                    Decimal(str(fee_detail.get("totalFee") or "0")),
                    commission_currency,
                ),
                liquidity_side=BitgetExecutionClient._parse_private_liquidity_side(
                    entry.get("tradeScope"),
                ),
                ts_event=millis_to_nanos(int(entry.get("uTime") or payload.get("ts") or 0)),
            )

    def _handle_positions_channel(self, payload: dict[str, Any]) -> None:
        data = payload.get("data") or []
        if not data:
            self._log.debug("Bitget private positions payload received: 0 entries")
            return

        arg = payload.get("arg") or {}
        inst_type = BitgetExecutionClient._product_type_key(arg.get("instType"))
        instruments_by_key: dict[tuple[str, str], Any] = {}
        for instrument_id in self._cache.instrument_ids(venue=BITGET_VENUE):
            instrument = self._cache.instrument(instrument_id)
            raw_symbol = getattr(getattr(instrument, "raw_symbol", None), "value", None)
            if instrument is not None and raw_symbol:
                key = (
                    BitgetExecutionClient._product_type_key(
                        BitgetExecutionClient._product_type_for_instrument(self, instrument),
                    ),
                    str(raw_symbol),
                )
                instruments_by_key[key] = instrument

        for entry in data:
            inst_id = str(entry.get("instId") or "").strip()
            instrument = instruments_by_key.get((inst_type, inst_id))
            if instrument is None:
                self._log.warning(
                    "Bitget private position update ignored: "
                    f"instrument not found for instType={inst_type} instId={inst_id}",
                )
                continue

            total = Decimal(str(entry.get("total") or "0"))
            ts_event = millis_to_nanos(int(entry.get("uTime") or payload.get("ts") or 0))
            venue_position_id = PositionId(str(entry["posId"])) if entry.get("posId") else None
            avg_px_raw = str(entry.get("openPriceAvg") or "").strip()
            avg_px_open = Decimal(avg_px_raw) if avg_px_raw and avg_px_raw != "0" else None

            if total == 0:
                report = PositionStatusReport(
                    account_id=self.account_id,
                    instrument_id=instrument.id,
                    position_side=PositionSide.FLAT,
                    quantity=Quantity.zero(instrument.size_precision),
                    venue_position_id=venue_position_id,
                    avg_px_open=None,
                    report_id=UUID4(),
                    ts_last=ts_event,
                    ts_init=ts_event,
                )
            else:
                hold_side = str(entry.get("holdSide") or "").strip().lower()
                position_side = PositionSide.SHORT if hold_side == "short" else PositionSide.LONG
                try:
                    quantity = instrument.make_qty(str(abs(total)), round_down=True)
                except TypeError:
                    quantity = instrument.make_qty(str(abs(total)))
                except ValueError:
                    quantity = Quantity.from_str(str(abs(total)))

                report = PositionStatusReport(
                    account_id=self.account_id,
                    instrument_id=instrument.id,
                    position_side=position_side,
                    quantity=quantity,
                    venue_position_id=venue_position_id,
                    avg_px_open=avg_px_open,
                    report_id=UUID4(),
                    ts_last=ts_event,
                    ts_init=ts_event,
                )

            self._send_position_status_report(report)

    async def _authenticate_ws(self) -> None:
        if (
            not self._config.api_key
            or not self._config.api_secret
            or not self._config.api_passphrase
        ):
            self._log.warning(
                "Bitget execution client missing private WebSocket credentials; skipping auth",
            )
            return

        await self._send_ws_text(
            nautilus_pyo3.BitgetWebSocketClient.login_message(
                self._config.api_key,
                self._config.api_passphrase,
                self._config.api_secret,
                time.time_ns() // 1_000_000,
            ),
        )

    async def _subscribe_private_ws(self) -> None:
        for product_type in self._product_types:
            await self._send_ws_text(
                nautilus_pyo3.BitgetWebSocketClient.subscribe_account_message(
                    product_type,
                    "default",
                ),
            )
            await self._send_ws_text(
                nautilus_pyo3.BitgetWebSocketClient.subscribe_message(
                    product_type,
                    "orders",
                    "default",
                ),
            )
            await self._send_ws_text(
                nautilus_pyo3.BitgetWebSocketClient.subscribe_message(
                    product_type,
                    "fill",
                    "default",
                ),
            )
            if not self._is_spot_product_type(product_type):
                await self._send_ws_text(
                    nautilus_pyo3.BitgetWebSocketClient.subscribe_message(
                        product_type,
                        "positions",
                        "default",
                    ),
                )

    def _is_spot_product_type(self, product_type: object) -> bool:
        if product_type == nautilus_pyo3.BitgetProductType.SPOT:
            return True

        name = getattr(product_type, "name", None)
        if isinstance(name, str) and name.upper() == "SPOT":
            return True

        value = getattr(product_type, "value", None)
        if isinstance(value, str) and value.upper() == "SPOT":
            return True

        return str(product_type).upper().endswith("SPOT")

    async def _send_ws_text(self, text: str) -> None:
        if self._ws_client is None:
            self._log.warning("Bitget private WebSocket not connected")
            return

        await self._ws_client.send_text(text.encode("utf-8"))

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        try:
            instrument = BitgetExecutionClient._resolve_instrument(self, command.instrument_id)
            if instrument is None:
                self._log.warning(f"Cannot find Bitget instrument for {command.instrument_id}")
                return None

            product_type = BitgetExecutionClient._product_type_for_instrument_id(
                self,
                command.instrument_id,
            )
            symbol = BitgetExecutionClient._raw_symbol_from_instrument_id(command.instrument_id)
            venue_order_id = command.venue_order_id
            if venue_order_id is None:
                venue_lookup = getattr(self._cache, "venue_order_id", None)
                if callable(venue_lookup) and command.client_order_id is not None:
                    venue_order_id = venue_lookup(command.client_order_id)

            payload = await self._http_client.request_order_status_report(
                product_type=product_type,
                symbol=symbol,
                margin_coin=BitgetExecutionClient._margin_coin_for_instrument_id(
                    self,
                    command.instrument_id,
                ),
                client_oid=command.client_order_id.value if command.client_order_id else None,
                order_id=venue_order_id.value if venue_order_id else None,
                account_mode=BitgetExecutionClient._account_mode_from_config(
                    getattr(self, "_config", None),
                ),
                allow_cash_borrowing=BitgetExecutionClient._allow_cash_borrowing_from_config(
                    getattr(self, "_config", None),
                ),
            )
            return BitgetExecutionClient._build_order_status_report(
                self,
                BitgetExecutionClient._parse_response_payload(payload),
                product_type,
                fallback_instrument_id=command.instrument_id,
                fallback_client_order_id=command.client_order_id,
                fallback_venue_order_id=venue_order_id,
            )
        except Exception as e:
            self._log.exception("Failed to generate OrderStatusReport", e)
            return None

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        reports: list[OrderStatusReport] = []
        start_ms = BitgetExecutionClient._datetime_to_millis(command.start)
        end_ms = BitgetExecutionClient._datetime_to_millis(command.end)
        ts_init = self._clock.timestamp_ns()

        try:
            if command.instrument_id is not None:
                product_types = [
                    BitgetExecutionClient._product_type_for_instrument_id(
                        self,
                        command.instrument_id,
                    ),
                ]
                symbol = BitgetExecutionClient._raw_symbol_from_instrument_id(command.instrument_id)
            else:
                product_types = list(self._product_types)
                symbol = None

            for product_type in product_types:
                payload = await self._http_client.request_order_status_reports(
                    product_type=product_type,
                    symbol=symbol,
                    margin_coin=(
                        BitgetExecutionClient._margin_coin_for_instrument_id(self, command.instrument_id)
                        if command.instrument_id is not None
                        else ("USDC" if BitgetExecutionClient._product_type_key(product_type) == "USDC-FUTURES" else None)
                    ),
                    open_only=command.open_only,
                    start=start_ms,
                    end=end_ms,
                    limit=None,
                    account_mode=BitgetExecutionClient._account_mode_from_config(
                        getattr(self, "_config", None),
                    ),
                    allow_cash_borrowing=BitgetExecutionClient._allow_cash_borrowing_from_config(
                        getattr(self, "_config", None),
                    ),
                )
                for item in BitgetExecutionClient._payload_items(payload):
                    report = BitgetExecutionClient._build_order_status_report(
                        self,
                        item,
                        product_type,
                        fallback_instrument_id=command.instrument_id,
                        ts_init=ts_init,
                    )
                    if report is not None:
                        reports.append(report)
        except Exception as e:
            self._log.exception("Failed to generate OrderStatusReports", e)

        return reports

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        reports: list[FillReport] = []
        start_ms = BitgetExecutionClient._datetime_to_millis(command.start)
        end_ms = BitgetExecutionClient._datetime_to_millis(command.end)
        ts_init = self._clock.timestamp_ns()

        try:
            if command.instrument_id is not None:
                product_types = [
                    BitgetExecutionClient._product_type_for_instrument_id(
                        self,
                        command.instrument_id,
                    ),
                ]
                symbol = BitgetExecutionClient._raw_symbol_from_instrument_id(command.instrument_id)
            else:
                product_types = list(self._product_types)
                symbol = None

            for product_type in product_types:
                payload = await self._http_client.request_fill_reports(
                    product_type=product_type,
                    symbol=symbol,
                    margin_coin=(
                        BitgetExecutionClient._margin_coin_for_instrument_id(self, command.instrument_id)
                        if command.instrument_id is not None
                        else ("USDC" if BitgetExecutionClient._product_type_key(product_type) == "USDC-FUTURES" else None)
                    ),
                    order_id=command.venue_order_id.value if command.venue_order_id else None,
                    start=start_ms,
                    end=end_ms,
                    limit=None,
                    account_mode=BitgetExecutionClient._account_mode_from_config(
                        getattr(self, "_config", None),
                    ),
                    allow_cash_borrowing=BitgetExecutionClient._allow_cash_borrowing_from_config(
                        getattr(self, "_config", None),
                    ),
                )
                for item in BitgetExecutionClient._payload_items(payload):
                    if (
                        command.venue_order_id is not None
                        and BitgetExecutionClient._string_value(
                            BitgetExecutionClient._field(item, "orderId"),
                        )
                        != command.venue_order_id.value
                    ):
                        continue
                    report = BitgetExecutionClient._build_fill_report(
                        self,
                        item,
                        product_type,
                        fallback_instrument_id=command.instrument_id,
                        ts_init=ts_init,
                    )
                    if report is not None:
                        reports.append(report)
        except Exception as e:
            self._log.exception("Failed to generate FillReports", e)

        return reports

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        reports: list[PositionStatusReport] = []
        ts_init = self._clock.timestamp_ns()

        try:
            if command.instrument_id is not None:
                product_types = [
                    BitgetExecutionClient._product_type_for_instrument_id(
                        self,
                        command.instrument_id,
                    ),
                ]
                symbol = BitgetExecutionClient._raw_symbol_from_instrument_id(command.instrument_id)
            else:
                product_types = [
                    product_type
                    for product_type in self._product_types
                    if not BitgetExecutionClient._is_spot_product_type(self, product_type)
                ]
                symbol = None

            for product_type in product_types:
                if BitgetExecutionClient._is_spot_product_type(self, product_type):
                    continue
                payload = await self._http_client.request_position_status_reports(
                    product_type=product_type,
                    symbol=symbol,
                    margin_coin=(
                        BitgetExecutionClient._margin_coin_for_instrument_id(self, command.instrument_id)
                        if command.instrument_id is not None
                        else ("USDC" if BitgetExecutionClient._product_type_key(product_type) == "USDC-FUTURES" else None)
                    ),
                    account_mode=BitgetExecutionClient._account_mode_from_config(
                        getattr(self, "_config", None),
                    ),
                )
                for item in BitgetExecutionClient._payload_items(payload):
                    report = BitgetExecutionClient._build_position_status_report(
                        self,
                        item,
                        product_type,
                        fallback_instrument_id=command.instrument_id,
                        ts_init=ts_init,
                    )
                    if report is not None:
                        reports.append(report)
        except Exception as e:
            self._log.exception("Failed to generate PositionStatusReports", e)

        return reports

    async def generate_mass_status(
        self, lookback_mins: int | None = None
    ) -> ExecutionMassStatus | None:
        self.reconciliation_active = True
        since = None
        if lookback_mins is not None:
            since = self._clock.utc_now() - timedelta(minutes=lookback_mins)

        try:
            order_reports, fill_reports, position_reports = await asyncio.gather(
                self.generate_order_status_reports(
                    GenerateOrderStatusReports(
                        instrument_id=None,
                        start=since,
                        end=None,
                        open_only=True,
                        command_id=UUID4(),
                        ts_init=self._clock.timestamp_ns(),
                    ),
                ),
                self.generate_fill_reports(
                    GenerateFillReports(
                        instrument_id=None,
                        venue_order_id=None,
                        start=since,
                        end=None,
                        command_id=UUID4(),
                        ts_init=self._clock.timestamp_ns(),
                    ),
                ),
                self.generate_position_status_reports(
                    GeneratePositionStatusReports(
                        instrument_id=None,
                        start=since,
                        end=None,
                        command_id=UUID4(),
                        ts_init=self._clock.timestamp_ns(),
                    ),
                ),
            )

            mass_status = ExecutionMassStatus(
                client_id=self.id,
                account_id=self.account_id
                or BitgetExecutionClient._default_account_id(self.id.value),
                venue=BITGET_VENUE,
                report_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )
            mass_status.add_order_reports(order_reports)
            mass_status.add_fill_reports(fill_reports)
            mass_status.add_position_reports(position_reports)
            return mass_status
        except Exception as e:
            self._log.exception("Cannot reconcile execution state", e)
            return None
        finally:
            self.reconciliation_active = False

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order
        if order.is_closed:
            self._log.warning(f"Cannot submit already closed order: {order}")
            return

        product_type = BitgetExecutionClient._product_type_for_instrument_id(
            self,
            order.instrument_id,
        )
        if reason := BitgetExecutionClient._check_order_validity(order, product_type):
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=reason,
                ts_event=self._clock.timestamp_ns(),
            )
            return

        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        try:
            payload = await self._http_client.submit_order(
                product_type=product_type,
                symbol=BitgetExecutionClient._raw_symbol_from_instrument_id(order.instrument_id),
                margin_coin=BitgetExecutionClient._margin_coin_for_instrument_id(
                    self,
                    order.instrument_id,
                ),
                client_oid=order.client_order_id.value,
                side=BitgetExecutionClient._order_side_to_api_str(order.side),
                order_type=BitgetExecutionClient._order_type_to_api_str(order.order_type),
                size=str(order.quantity),
                force=BitgetExecutionClient._time_in_force_to_api_force(order),
                price=str(order.price) if order.has_price else None,
                reduce_only=order.is_reduce_only,
                account_mode=BitgetExecutionClient._account_mode_from_config(
                    getattr(self, "_config", None),
                ),
                allow_cash_borrowing=BitgetExecutionClient._allow_cash_borrowing_from_config(
                    getattr(self, "_config", None),
                ),
                margin_mode=BitgetExecutionClient._margin_mode_from_config(
                    getattr(self, "_config", None),
                ),
                position_mode=BitgetExecutionClient._position_mode_from_config(
                    getattr(self, "_config", None),
                ),
            )
            payload = BitgetExecutionClient._parse_response_payload(payload)
            order_id = BitgetExecutionClient._string_value(
                BitgetExecutionClient._field(payload, "orderId"),
            )
            if order_id:
                self._cache.add_venue_order_id(order.client_order_id, VenueOrderId(order_id))
        except Exception as e:
            self._log.error(f"Failed to submit order {order.client_order_id}: {e}")
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=BitgetExecutionClient._format_exchange_error_reason(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        if not command.order_list.orders:
            return

        for order in command.order_list.orders:
            await BitgetExecutionClient._submit_order(
                self,
                SimpleNamespace(order=order, params=command.params),
            )

    async def _modify_order(self, command: ModifyOrder) -> None:
        order = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id!r} not found in cache")
            return

        if order.is_closed:
            self._log.warning(
                f"`ModifyOrder` command for {command.client_order_id!r} when order already {order.status_string()} "
                "(will not send to exchange)",
            )
            return

        product_type = BitgetExecutionClient._product_type_for_instrument_id(
            self,
            command.instrument_id,
        )
        if order.order_type != OrderType.LIMIT:
            self.generate_order_modify_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason="Bitget only supports modify for limit orders",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        venue_order_id = command.venue_order_id
        if venue_order_id is None:
            venue_lookup = getattr(self._cache, "venue_order_id", None)
            if callable(venue_lookup):
                venue_order_id = venue_lookup(command.client_order_id)
        new_client_oid = None
        if BitgetExecutionClient._product_type_key(product_type) != "SPOT":
            new_client_oid = f"{command.client_order_id.value}-MOD-{int(time.time() * 1000)}"

        try:
            payload = await self._http_client.modify_order(
                product_type=product_type,
                symbol=BitgetExecutionClient._raw_symbol_from_instrument_id(command.instrument_id),
                margin_coin=BitgetExecutionClient._margin_coin_for_instrument_id(
                    self,
                    command.instrument_id,
                ),
                client_oid=command.client_order_id.value,
                order_id=venue_order_id.value if venue_order_id else None,
                new_client_oid=new_client_oid,
                size=str(command.quantity) if command.quantity is not None else None,
                price=str(command.price) if command.price is not None else None,
            )
            payload = BitgetExecutionClient._parse_response_payload(payload)
            order_id = BitgetExecutionClient._string_value(
                BitgetExecutionClient._field(payload, "orderId"),
            )
            if order_id:
                self._cache.add_venue_order_id(order.client_order_id, VenueOrderId(order_id))
        except Exception as e:
            self._log.error(f"Failed to modify order {command.client_order_id}: {e}")
            self.generate_order_modify_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _cancel_order(self, command: CancelOrder) -> None:
        order = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id!r} not found in cache")
            return

        if order.is_closed:
            self._log.warning(
                f"`CancelOrder` command for {command.client_order_id!r} when order already {order.status_string()} "
                "(will not send to exchange)",
            )
            return

        venue_order_id = command.venue_order_id
        if venue_order_id is None:
            venue_lookup = getattr(self._cache, "venue_order_id", None)
            if callable(venue_lookup):
                venue_order_id = venue_lookup(command.client_order_id)
        try:
            await self._http_client.cancel_order(
                product_type=BitgetExecutionClient._product_type_for_instrument_id(
                    self,
                    command.instrument_id,
                ),
                symbol=BitgetExecutionClient._raw_symbol_from_instrument_id(command.instrument_id),
                margin_coin=BitgetExecutionClient._margin_coin_for_instrument_id(
                    self,
                    command.instrument_id,
                ),
                client_oid=command.client_order_id.value,
                order_id=venue_order_id.value if venue_order_id else None,
                account_mode=BitgetExecutionClient._account_mode_from_config(
                    getattr(self, "_config", None),
                ),
                allow_cash_borrowing=BitgetExecutionClient._allow_cash_borrowing_from_config(
                    getattr(self, "_config", None),
                ),
            )
        except Exception as e:
            self._log.error(f"Failed to cancel order {command.client_order_id}: {e}")
            self.generate_order_cancel_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=BitgetExecutionClient._format_exchange_error_reason(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        if command.order_side != OrderSide.NO_ORDER_SIDE:
            self._log.warning(
                "Bitget does not support order_side filtering for cancel all orders; "
                "ignoring filter and canceling all orders for the instrument",
            )

        try:
            await self._http_client.cancel_all_orders(
                product_type=BitgetExecutionClient._product_type_for_instrument_id(
                    self,
                    command.instrument_id,
                ),
                symbol=BitgetExecutionClient._raw_symbol_from_instrument_id(command.instrument_id),
                margin_coin=BitgetExecutionClient._margin_coin_for_instrument_id(
                    self,
                    command.instrument_id,
                ),
                account_mode=BitgetExecutionClient._account_mode_from_config(
                    getattr(self, "_config", None),
                ),
                allow_cash_borrowing=BitgetExecutionClient._allow_cash_borrowing_from_config(
                    getattr(self, "_config", None),
                ),
            )
        except Exception as e:
            self._log.error(f"Failed to cancel all orders for {command.instrument_id}: {e}")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        if not command.cancels:
            return

        grouped: dict[tuple[str, str], list[Any]] = {}
        product_types_by_group: dict[tuple[str, str], Any] = {}
        for cancel in command.cancels:
            product_type = BitgetExecutionClient._product_type_for_instrument_id(
                self,
                cancel.instrument_id,
            )
            symbol = BitgetExecutionClient._raw_symbol_from_instrument_id(cancel.instrument_id)
            key = (BitgetExecutionClient._product_type_key(product_type), symbol)
            product_types_by_group.setdefault(key, product_type)
            grouped.setdefault(key, []).append(cancel)

        for key, cancels in grouped.items():
            _, symbol = key
            product_type = product_types_by_group[key]
            client_oids = [
                cancel.client_order_id.value
                for cancel in cancels
                if cancel.client_order_id is not None and cancel.venue_order_id is None
            ]
            order_ids = [
                cancel.venue_order_id.value
                for cancel in cancels
                if cancel.venue_order_id is not None
            ]

            try:
                await self._http_client.batch_cancel_orders(
                    product_type=product_type,
                    symbol=symbol,
                    margin_coin=(
                        BitgetExecutionClient._margin_coin_for_instrument_id(self, cancels[0].instrument_id)
                        if cancels
                        else None
                    ),
                    client_oids=client_oids,
                    order_ids=order_ids,
                )
            except Exception as e:
                self._log.error(f"Failed to batch cancel orders for {symbol}: {e}")
                for cancel in cancels:
                    order = self._cache.order(cancel.client_order_id)
                    if order is not None and not order.is_closed:
                        self.generate_order_cancel_rejected(
                            strategy_id=order.strategy_id,
                            instrument_id=order.instrument_id,
                            client_order_id=order.client_order_id,
                            venue_order_id=order.venue_order_id,
                            reason=str(e),
                            ts_event=self._clock.timestamp_ns(),
                        )

    async def _query_account(self, command: QueryAccount) -> None:
        self._log.debug(f"Query account not implemented: {command}")
