"""Live execution client for Rithmic."""

from __future__ import annotations

import asyncio
import json
from decimal import Decimal
from pathlib import Path
from typing import TYPE_CHECKING, Optional

from nautilus_trader.execution.messages import (
    BatchCancelOrders,
    CancelAllOrders,
    CancelOrder,
    ModifyOrder,
    SubmitOrder,
    SubmitOrderList,
)
from nautilus_trader.execution.reports import (
    FillReport,
    OrderStatusReport,
    PositionStatusReport,
)
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.enums import (
    AccountType,
    ContingencyType,
    LiquiditySide,
    OmsType,
    OrderSide,
    OrderStatus,
    OrderType,
    PositionSide,
    TimeInForce,
    TriggerType,
)
from nautilus_trader.model.identifiers import (
    AccountId,
    ClientId,
    ClientOrderId,
    InstrumentId,
    TradeId,
    Venue,
    VenueOrderId,
)
from nautilus_trader.model.objects import Currency, Money, Price, Quantity
from nautilus_trader.model.objects import AccountBalance as NautilusAccountBalance
from nautilus_trader.common.providers import InstrumentProvider

from nautilus_trader.adapters.rithmic.bindings import (
    AccountEvent,
    OrderSide as RithmicOrderSide,
    OrderType as RithmicOrderType,
    PositionEvent,
    RithmicExecutionClient,
    RithmicGateway,
    TimeInForce as RithmicTimeInForce,
)
from nautilus_trader.adapters.rithmic.config import RithmicExecClientConfig

if TYPE_CHECKING:
    from nautilus_trader.cache import Cache
    from nautilus_trader.common.component import MessageBus
    from nautilus_trader.execution.messages import (
        GenerateFillReports,
        GenerateOrderStatusReport,
        GenerateOrderStatusReports,
        GeneratePositionStatusReports,
    )
    from nautilus_trader.execution.reports import ExecutionMassStatus


RITHMIC_VENUE = Venue("RITHMIC")
_HANDLER_EXCEPTIONS = (AttributeError, KeyError, LookupError, RuntimeError, TypeError, ValueError)


class RithmicLiveExecutionClient(LiveExecutionClient):
    """
    Provides a live execution client for Rithmic.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client_id : ClientId
        The client ID.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    config : RithmicExecClientConfig
        The configuration for the client.
    """

    def __init__(
        self,
        loop,
        client_id,
        msgbus: "MessageBus",
        cache: "Cache",
        clock,
        config: RithmicExecClientConfig,
        instrument_provider=None,
    ) -> None:
        if not isinstance(client_id, ClientId):
            client_id = ClientId(str(client_id))

        provider = instrument_provider or InstrumentProvider(
            config.instrument_provider if hasattr(config, "instrument_provider") else None
        )

        super().__init__(
            loop=loop,
            client_id=client_id,
            venue=RITHMIC_VENUE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=provider,
            config=config,
        )
        self._config = config
        self._set_account_id(AccountId(f"{RITHMIC_VENUE.value}-{config.account_id}"))
        self._gateway: RithmicGateway | None = None
        self._client: RithmicExecutionClient | None = None
        self._orders: dict[str, dict] = {}
        self._fills: list[dict] = []
        self._positions: dict[str, dict] = {}
        self._pnl_task = None
        self._balances: dict[str, dict] = {}
        self._pnl_live = False
        self._accessible_accounts: list[str] = []
        self._primary_balance_event = asyncio.Event()
        self._seen_fill_keys: set[tuple[str, str, str, int, str, str]] = set()
        self._native_brackets: dict[str, dict] = {}
        self._native_brackets_by_parent_venue_id: dict[str, str] = {}
        self._native_bracket_order_seeds: dict[str, dict] = {}
        self._load_native_bracket_state()

    def _require_account_id(self) -> AccountId:
        if self.account_id is None:
            raise RuntimeError("Execution account not initialized")
        return self.account_id

    def _assert_order_account_scope(self, order) -> None:
        account_id = getattr(order, "account_id", None)
        if account_id is None:
            return

        issuer = None
        get_issuer = getattr(account_id, "get_issuer", None)
        if callable(get_issuer):
            try:
                issuer = get_issuer()
            except Exception:
                issuer = None

        if issuer != RITHMIC_VENUE.value:
            return

        account_text = self._identifier_text(account_id)
        expected = self._require_account_id().value
        if account_text is None or account_text == expected:
            return

        raise ValueError(
            f"Order account_id {account_text!r} does not match configured Rithmic execution client "
            f"account {expected!r}. Rithmic execution remains one client and one connection per account."
        )

    @staticmethod
    def _to_decimal(value) -> Decimal | None:
        if value is None:
            return None
        if isinstance(value, Decimal):
            return value
        return Decimal(str(value))

    @classmethod
    def _decimal_str(cls, value) -> str:
        decimal_value = cls._to_decimal(value)
        if decimal_value is None:
            raise ValueError("Decimal value required")
        text = format(decimal_value, "f")
        if "." in text:
            text = text.rstrip("0").rstrip(".")
        return text or "0"

    @classmethod
    def _to_quantity(cls, value) -> Quantity:
        return Quantity.from_str(cls._decimal_str(value))

    @classmethod
    def _to_price(cls, value) -> Price | None:
        if value is None:
            return None
        return Price.from_str(str(value))

    @staticmethod
    def _to_currency(code: str | None) -> Currency:
        return Currency.from_str(code or "USD")

    def _signal_primary_balance(self) -> None:
        try:
            self._loop.call_soon_threadsafe(self._primary_balance_event.set)
        except RuntimeError:
            # The loop is shutting down.
            pass

    async def _wait_for_primary_balance(self, timeout_secs: float = 10.0) -> None:
        if self._config.account_id in self._balances:
            return

        self._primary_balance_event.clear()
        await asyncio.wait_for(self._primary_balance_event.wait(), timeout=timeout_secs)

        if self._config.account_id not in self._balances:
            raise RuntimeError(
                f"Did not receive account state for {self._config.account_id!r} "
                f"within {timeout_secs}s"
            )

    def _refresh_account_state(
        self,
        account_id: str | None = None,
        ts_event: int | None = None,
    ) -> None:
        target_account = account_id or self._config.account_id
        if target_account != self._config.account_id:
            return

        balance = self._balances.get(target_account)
        if balance is None:
            raise RuntimeError(f"Account balance for {target_account!r} is not available")

        currency = self._to_currency(balance.get("currency"))
        total_value = self._to_decimal(balance.get("total", 0.0)) or Decimal("0")
        locked_value = self._to_decimal(balance.get("locked", 0.0)) or Decimal("0")
        free_value = total_value - locked_value
        total = Money(float(total_value), currency)
        free = Money(float(free_value), currency)
        locked = Money(float(locked_value), currency)

        self.generate_account_state(
            balances=[
                NautilusAccountBalance(
                    total=total,
                    locked=locked,
                    free=free,
                ),
            ],
            margins=[],
            reported=True,
            ts_event=ts_event or self._clock.timestamp_ns(),
            info={
                "accessible_accounts": list(self._accessible_accounts),
                "currency": currency.code,
                "available": str(balance.get("available", Decimal("0"))),
                "locked": str(balance.get("locked", Decimal("0"))),
                "realized_pnl": str(balance.get("realized_pnl", Decimal("0"))),
                "unrealized_pnl": str(balance.get("unrealized_pnl", Decimal("0"))),
            },
        )

    @staticmethod
    def _to_rithmic_side(side: OrderSide) -> RithmicOrderSide:
        if side == OrderSide.BUY:
            return RithmicOrderSide.BUY
        if side == OrderSide.SELL:
            return RithmicOrderSide.SELL
        raise ValueError(f"Unsupported order side for Rithmic: {side}")

    @staticmethod
    def _to_rithmic_order_type(order_type: OrderType) -> RithmicOrderType:
        if order_type == OrderType.MARKET:
            return RithmicOrderType.MARKET
        if order_type == OrderType.LIMIT:
            return RithmicOrderType.LIMIT
        if order_type == OrderType.STOP_MARKET:
            return RithmicOrderType.STOP_MARKET
        if order_type == OrderType.STOP_LIMIT:
            return RithmicOrderType.STOP_LIMIT
        raise ValueError(f"Unsupported order type for Rithmic: {order_type}")

    @staticmethod
    def _to_rithmic_tif(time_in_force: TimeInForce) -> RithmicTimeInForce:
        if time_in_force == TimeInForce.DAY:
            return RithmicTimeInForce.DAY
        if time_in_force == TimeInForce.GTC:
            return RithmicTimeInForce.GTC
        if time_in_force == TimeInForce.IOC:
            return RithmicTimeInForce.IOC
        if time_in_force == TimeInForce.FOK:
            return RithmicTimeInForce.FOK
        raise ValueError(f"Unsupported time in force for Rithmic: {time_in_force}")

    @staticmethod
    def _position_side(quantity: Decimal) -> PositionSide:
        if quantity > 0:
            return PositionSide.LONG
        if quantity < 0:
            return PositionSide.SHORT
        return PositionSide.FLAT

    @staticmethod
    def _order_status(value) -> OrderStatus:
        if isinstance(value, OrderStatus):
            return value

        mapping = {
            "PENDING": OrderStatus.SUBMITTED,
            "OPEN": OrderStatus.ACCEPTED,
            "PARTIAL": OrderStatus.PARTIALLY_FILLED,
            "COMPLETE": OrderStatus.FILLED,
            "CANCELLED": OrderStatus.CANCELED,
            "REJECTED": OrderStatus.REJECTED,
        }
        return mapping.get(str(value), OrderStatus.INITIALIZED)

    @staticmethod
    def _is_open_order_status(status: OrderStatus) -> bool:
        return status in {
            OrderStatus.INITIALIZED,
            OrderStatus.SUBMITTED,
            OrderStatus.ACCEPTED,
            OrderStatus.TRIGGERED,
            OrderStatus.PENDING_UPDATE,
            OrderStatus.PENDING_CANCEL,
            OrderStatus.PARTIALLY_FILLED,
        }

    @staticmethod
    def _datetime_to_ns(value) -> int | None:
        if value is None:
            return None
        if hasattr(value, "value"):
            return int(value.value)
        if hasattr(value, "timestamp"):
            return int(value.timestamp() * 1_000_000_000)
        return None

    def _find_order_state(
        self,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
    ) -> dict | None:
        if client_order_id is not None:
            state = self._orders.get(client_order_id.value)
            if state is not None:
                return state

        if venue_order_id is None:
            return None

        for state in self._orders.values():
            existing = state.get("venue_order_id")
            if isinstance(existing, VenueOrderId) and existing == venue_order_id:
                return state
        return None

    def _tracked_venue_order_id(self, client_order_id: str) -> VenueOrderId | None:
        if not self._client or not hasattr(self._client, "get_order"):
            return None

        tracked = self._client.get_order(client_order_id)
        if not tracked:
            return None

        venue_order_id = tracked.get("venue_order_id")
        if not venue_order_id:
            return None

        return VenueOrderId(venue_order_id)

    def _seed_order_state(
        self,
        order,
        *,
        status: OrderStatus,
        venue_order_id: VenueOrderId | None = None,
    ) -> dict:
        now_ns = self._clock.timestamp_ns()
        quantity = self._to_decimal(order.quantity) or Decimal("0")
        state = {
            "client_order_id": order.client_order_id,
            "instrument_id": order.instrument_id,
            "order_side": order.side,
            "order_type": order.order_type,
            "time_in_force": order.time_in_force,
            "quantity": quantity,
            "filled_qty": Decimal("0"),
            "leaves_qty": quantity,
            "price": str(getattr(order, "price", None)) if getattr(order, "price", None) is not None else None,
            "trigger_price": (
                str(getattr(order, "trigger_price", None))
                if getattr(order, "trigger_price", None) is not None
                else None
            ),
            "order_list_id": getattr(order, "order_list_id", None),
            "linked_order_ids": getattr(order, "linked_order_ids", None),
            "parent_order_id": getattr(order, "parent_order_id", None),
            "contingency_type": getattr(order, "contingency_type", ContingencyType.NO_CONTINGENCY),
            "expire_time": getattr(order, "expire_time", None),
            "display_qty": getattr(order, "display_qty", None),
            "post_only": bool(getattr(order, "post_only", False)),
            "reduce_only": bool(getattr(order, "reduce_only", False)),
            "status": status,
            "ts_accepted": 0,
            "ts_last": 0 if status in {OrderStatus.INITIALIZED, OrderStatus.SUBMITTED} else getattr(order, "ts_init", now_ns),
            "ts_init": getattr(order, "ts_init", now_ns),
            "venue_order_id": venue_order_id or getattr(order, "venue_order_id", None),
        }
        self._orders[order.client_order_id.value] = state
        return state

    @staticmethod
    def _identifier_text(value) -> str | None:
        if value is None:
            return None
        if hasattr(value, "value"):
            return str(value.value)
        return str(value)

    @staticmethod
    def _enum_name(value) -> str | None:
        if value is None:
            return None
        return getattr(value, "name", str(value))

    @staticmethod
    def _safe_state_path_token(value: str) -> str:
        token = "".join(
            char if char.isalnum() or char in {"-", "_"} else "_"
            for char in str(value)
        )
        return token or "default"

    def _native_bracket_state_path(self) -> Path:
        configured = getattr(self._config, "native_bracket_state_path", None)
        if configured:
            return Path(configured).expanduser()

        env_name = self._safe_state_path_token(getattr(self._config.environment, "value", "demo"))
        account_id = self._safe_state_path_token(self._config.account_id)
        return (
            Path.home()
            / ".nautilus"
            / "rithmic"
            / "native_brackets"
            / f"{env_name}_{account_id}.json"
        )

    def _serialize_native_bracket_order_seed(self, order) -> dict:
        linked_order_ids = [
            linked.value if hasattr(linked, "value") else str(linked)
            for linked in (getattr(order, "linked_order_ids", None) or [])
        ]
        return {
            "instrument_id": self._identifier_text(getattr(order, "instrument_id", None)),
            "order_side": self._enum_name(getattr(order, "side", None)),
            "order_type": self._enum_name(getattr(order, "order_type", None)),
            "time_in_force": self._enum_name(getattr(order, "time_in_force", None)),
            "quantity": (
                self._decimal_str(getattr(order, "quantity", None))
                if getattr(order, "quantity", None) is not None
                else None
            ),
            "price": (
                self._decimal_str(getattr(order, "price", None))
                if getattr(order, "price", None) is not None
                else None
            ),
            "trigger_price": (
                self._decimal_str(getattr(order, "trigger_price", None))
                if getattr(order, "trigger_price", None) is not None
                else None
            ),
            "linked_order_ids": linked_order_ids,
            "parent_order_id": self._identifier_text(getattr(order, "parent_order_id", None)),
            "contingency_type": self._enum_name(getattr(order, "contingency_type", None)),
            "post_only": bool(getattr(order, "post_only", False)),
            "reduce_only": bool(getattr(order, "reduce_only", False)),
            "ts_init": int(getattr(order, "ts_init", 0) or 0),
        }

    def _apply_native_bracket_order_seed(self, client_order_id: str, state: dict) -> None:
        seed = self._native_bracket_order_seeds.get(client_order_id)
        if not seed:
            return

        instrument_id = seed.get("instrument_id")
        if instrument_id and state.get("instrument_id") is None:
            state["instrument_id"] = InstrumentId.from_str(instrument_id)

        side = seed.get("order_side")
        if side and state.get("order_side") is None:
            state["order_side"] = getattr(OrderSide, side, OrderSide.NO_ORDER_SIDE)

        order_type = seed.get("order_type")
        if order_type and state.get("order_type") is None:
            state["order_type"] = getattr(OrderType, order_type, OrderType.MARKET)

        time_in_force = seed.get("time_in_force")
        if time_in_force and state.get("time_in_force") is None:
            state["time_in_force"] = getattr(TimeInForce, time_in_force, TimeInForce.DAY)

        quantity = seed.get("quantity")
        if quantity is not None and state.get("quantity") is None:
            state["quantity"] = Decimal(str(quantity))

        price = seed.get("price")
        if price is not None and state.get("price") is None:
            state["price"] = str(price)

        trigger_price = seed.get("trigger_price")
        if trigger_price is not None and state.get("trigger_price") is None:
            state["trigger_price"] = str(trigger_price)

        linked_order_ids = seed.get("linked_order_ids") or []
        if linked_order_ids and state.get("linked_order_ids") is None:
            state["linked_order_ids"] = [ClientOrderId(value) for value in linked_order_ids]

        parent_order_id = seed.get("parent_order_id")
        if parent_order_id and state.get("parent_order_id") is None:
            state["parent_order_id"] = ClientOrderId(parent_order_id)

        contingency_type = seed.get("contingency_type")
        if contingency_type and state.get("contingency_type") is None:
            state["contingency_type"] = getattr(
                ContingencyType,
                contingency_type,
                ContingencyType.NO_CONTINGENCY,
            )

        state.setdefault("post_only", bool(seed.get("post_only", False)))
        state.setdefault("reduce_only", bool(seed.get("reduce_only", False)))
        state.setdefault("ts_init", int(seed.get("ts_init", 0) or self._clock.timestamp_ns()))

    def _save_native_bracket_state(self) -> None:
        path = self._native_bracket_state_path()
        try:
            if not self._native_brackets:
                path.unlink(missing_ok=True)
                return

            path.parent.mkdir(parents=True, exist_ok=True)
            payload = {
                "version": 1,
                "account_id": self._config.account_id,
                "environment": getattr(self._config.environment, "value", "demo"),
                "brackets": [],
            }
            for parent_client_order_id in sorted(self._native_brackets):
                registry = self._native_brackets[parent_client_order_id]
                payload["brackets"].append(
                    {
                        "parent_client_order_id": parent_client_order_id,
                        "stop_client_order_id": registry.get("stop_client_order_id"),
                        "target_client_order_id": registry.get("target_client_order_id"),
                        "parent_venue_order_id": registry.get("parent_venue_order_id"),
                        "order_seeds": {
                            client_order_id: self._native_bracket_order_seeds.get(client_order_id, {})
                            for client_order_id in (
                                parent_client_order_id,
                                registry.get("stop_client_order_id"),
                                registry.get("target_client_order_id"),
                            )
                            if client_order_id
                        },
                    }
                )

            temp_path = path.with_name(f"{path.name}.tmp")
            temp_path.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")
            temp_path.replace(path)
        except (OSError, TypeError, ValueError) as exc:
            self._log.warning(
                f"Failed to persist native bracket state to {path}: {exc}"
            )

    def _load_native_bracket_state(self) -> None:
        path = self._native_bracket_state_path()
        self._native_brackets.clear()
        self._native_brackets_by_parent_venue_id.clear()
        self._native_bracket_order_seeds.clear()

        if not path.exists():
            return

        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            self._log.warning(f"Failed to load native bracket state from {path}: {exc}")
            return

        brackets = payload.get("brackets", [])
        if not isinstance(brackets, list):
            self._log.warning(f"Ignoring invalid native bracket state payload in {path}")
            return

        for entry in brackets:
            if not isinstance(entry, dict):
                continue

            parent_client_order_id = entry.get("parent_client_order_id")
            stop_client_order_id = entry.get("stop_client_order_id")
            target_client_order_id = entry.get("target_client_order_id")
            if not parent_client_order_id or not stop_client_order_id or not target_client_order_id:
                continue

            registry = {
                "parent_client_order_id": parent_client_order_id,
                "stop_client_order_id": stop_client_order_id,
                "target_client_order_id": target_client_order_id,
                "parent_venue_order_id": entry.get("parent_venue_order_id"),
            }
            self._native_brackets[parent_client_order_id] = registry

            parent_venue_order_id = registry.get("parent_venue_order_id")
            if parent_venue_order_id:
                self._native_brackets_by_parent_venue_id[parent_venue_order_id] = parent_client_order_id

            order_seeds = entry.get("order_seeds", {})
            if isinstance(order_seeds, dict):
                for client_order_id, seed in order_seeds.items():
                    if isinstance(client_order_id, str) and isinstance(seed, dict):
                        self._native_bracket_order_seeds[client_order_id] = seed

    def _remove_native_bracket(self, parent_client_order_id: str) -> None:
        registry = self._native_brackets.pop(parent_client_order_id, None)
        if registry is None:
            return

        parent_venue_order_id = registry.get("parent_venue_order_id")
        if parent_venue_order_id:
            self._native_brackets_by_parent_venue_id.pop(parent_venue_order_id, None)

        for client_order_id in (
            parent_client_order_id,
            registry.get("stop_client_order_id"),
            registry.get("target_client_order_id"),
        ):
            if client_order_id:
                self._native_bracket_order_seeds.pop(client_order_id, None)

        self._save_native_bracket_state()

    def _native_bracket_parent_client_order_id(self, client_order_id: str) -> str | None:
        if client_order_id in self._native_brackets:
            return client_order_id

        for parent_client_order_id, registry in self._native_brackets.items():
            if client_order_id in {
                registry.get("stop_client_order_id"),
                registry.get("target_client_order_id"),
            }:
                return parent_client_order_id
        return None

    def _maybe_retire_native_bracket(self, parent_client_order_id: str) -> None:
        registry = self._native_brackets.get(parent_client_order_id)
        if registry is None:
            return

        for client_order_id in (
            parent_client_order_id,
            registry.get("stop_client_order_id"),
            registry.get("target_client_order_id"),
        ):
            if not client_order_id:
                return
            state = self._orders.get(client_order_id)
            if state is None:
                return
            status = self._order_status(state.get("status"))
            if self._is_open_order_status(status):
                return

        self._remove_native_bracket(parent_client_order_id)

    def _maybe_retire_native_bracket_by_client_order_id(self, client_order_id: str) -> None:
        parent_client_order_id = self._native_bracket_parent_client_order_id(client_order_id)
        if parent_client_order_id is not None:
            self._maybe_retire_native_bracket(parent_client_order_id)

    @staticmethod
    def _native_bracket_role(bracket_type: str | None) -> str | None:
        if not bracket_type:
            return None
        if bracket_type.startswith("STOP_ONLY"):
            return "stop"
        if bracket_type.startswith("TARGET_ONLY"):
            return "target"
        return None

    def _native_bracket_registry(self, payload) -> dict[str, str | None] | None:
        candidates = [
            getattr(payload, "original_basket_id", None),
            getattr(payload, "client_order_id", None),
        ]
        candidates.extend(getattr(payload, "linked_basket_ids", None) or [])

        for candidate in candidates:
            if not candidate:
                continue
            if candidate in self._native_brackets:
                return self._native_brackets[candidate]
            parent_client_order_id = self._native_brackets_by_parent_venue_id.get(candidate)
            if parent_client_order_id:
                return self._native_brackets.get(parent_client_order_id)
        return None

    def _update_native_bracket_parent_venue_id(
        self,
        parent_client_order_id: str,
        venue_order_id: str | None,
    ) -> None:
        if not venue_order_id:
            return

        existing = self._native_brackets.get(parent_client_order_id)
        if existing is None:
            return

        prior = existing.get("parent_venue_order_id")
        if prior:
            self._native_brackets_by_parent_venue_id.pop(prior, None)

        existing["parent_venue_order_id"] = venue_order_id
        self._native_brackets_by_parent_venue_id[venue_order_id] = parent_client_order_id
        self._save_native_bracket_state()

    def _register_native_bracket(
        self,
        parent_order,
        stop_order,
        target_order,
        parent_venue_order_id: str | None = None,
    ) -> None:
        parent_client_order_id = parent_order.client_order_id.value
        self._native_brackets[parent_client_order_id] = {
            "parent_client_order_id": parent_client_order_id,
            "stop_client_order_id": stop_order.client_order_id.value,
            "target_client_order_id": target_order.client_order_id.value,
            "parent_venue_order_id": parent_venue_order_id,
        }
        self._native_bracket_order_seeds[parent_client_order_id] = self._serialize_native_bracket_order_seed(parent_order)
        self._native_bracket_order_seeds[stop_order.client_order_id.value] = self._serialize_native_bracket_order_seed(stop_order)
        self._native_bracket_order_seeds[target_order.client_order_id.value] = self._serialize_native_bracket_order_seed(target_order)
        if parent_venue_order_id:
            self._native_brackets_by_parent_venue_id[parent_venue_order_id] = parent_client_order_id
        self._save_native_bracket_state()

    def _propagate_native_bracket_parent_terminal(
        self,
        parent_client_order_id: str,
        *,
        status: OrderStatus,
        ts_event: int,
        reason: str | None = None,
    ) -> None:
        registry = self._native_brackets.get(parent_client_order_id)
        if registry is None:
            return

        for key in ("stop_client_order_id", "target_client_order_id"):
            child_client_order_id = registry.get(key)
            if not child_client_order_id:
                continue

            state = self._orders.setdefault(child_client_order_id, {})
            state.setdefault("client_order_id", ClientOrderId(child_client_order_id))
            state.setdefault("ts_init", ts_event)
            if self._is_stale_order_event(state, ts_event):
                continue

            state["status"] = status
            state["ts_last"] = ts_event
            if status == OrderStatus.CANCELED:
                state["leaves_qty"] = Decimal("0")
            if reason is not None:
                state["cancel_reason"] = reason

    def _resolve_event_client_order_id(self, payload) -> str:
        venue_order_id = getattr(payload, "venue_order_id", None)
        if venue_order_id:
            state = self._find_order_state(venue_order_id=VenueOrderId(venue_order_id))
            if state is not None:
                existing = state.get("client_order_id")
                if isinstance(existing, ClientOrderId):
                    return existing.value

        registry = self._native_bracket_registry(payload)
        role = self._native_bracket_role(getattr(payload, "bracket_type", None))
        if registry is not None and role is not None:
            resolved = registry.get(f"{role}_client_order_id")
            if resolved:
                return resolved

        fallback = getattr(payload, "client_order_id", None)
        parent_client_order_id = self._native_brackets_by_parent_venue_id.get(fallback)
        if parent_client_order_id:
            return parent_client_order_id
        return fallback

    def _find_instrument(self, instrument_id: InstrumentId):
        instrument = self._cache.instrument(instrument_id)
        if instrument is not None:
            return instrument

        provider = getattr(self, "_instrument_provider", None)
        if provider is None:
            provider = getattr(self, "instrument_provider", None)
        if provider is None:
            return None
        return provider.find(instrument_id)

    @classmethod
    def _tick_distance(cls, reference_price, other_price, price_increment, label: str) -> int:
        reference = cls._to_decimal(reference_price)
        other = cls._to_decimal(other_price)
        increment = cls._to_decimal(price_increment)
        if reference is None or other is None or increment is None or increment <= 0:
            raise ValueError(f"Cannot compute {label} ticks without reference prices and price_increment")

        delta = other - reference
        if delta <= 0:
            raise ValueError(f"{label} must be greater than the entry price for this bracket side")

        ticks = delta / increment
        if ticks != ticks.to_integral_value():
            raise ValueError(
                f"{label} distance {delta} is not aligned to instrument price increment {increment}"
            )
        return int(ticks)

    @staticmethod
    def _event_order_side(value: str | None) -> OrderSide | None:
        if value == "BUY":
            return OrderSide.BUY
        if value == "SELL":
            return OrderSide.SELL
        return None

    @staticmethod
    def _event_order_type(value: str | None) -> OrderType | None:
        mapping = {
            "MARKET": OrderType.MARKET,
            "LIMIT": OrderType.LIMIT,
            "STOP_MARKET": OrderType.STOP_MARKET,
            "STOP_LIMIT": OrderType.STOP_LIMIT,
        }
        return mapping.get(value)

    @staticmethod
    def _event_time_in_force(value: str | None) -> TimeInForce | None:
        mapping = {
            "DAY": TimeInForce.DAY,
            "GTC": TimeInForce.GTC,
            "IOC": TimeInForce.IOC,
            "FOK": TimeInForce.FOK,
        }
        return mapping.get(value)

    @staticmethod
    def _event_instrument_id(symbol: str | None) -> InstrumentId | None:
        if not symbol:
            return None
        return InstrumentId.from_str(f"{symbol}.{RITHMIC_VENUE.value}")

    @classmethod
    def _apply_execution_context(cls, state: dict, payload) -> None:
        instrument_id = cls._event_instrument_id(getattr(payload, "symbol", None))
        if instrument_id is not None:
            state["instrument_id"] = instrument_id

        exchange = getattr(payload, "exchange", None)
        if exchange:
            state["exchange"] = exchange

        side = cls._event_order_side(getattr(payload, "side", None))
        if side is not None:
            state["order_side"] = side

        order_type = cls._event_order_type(getattr(payload, "order_type", None))
        if order_type is not None:
            state["order_type"] = order_type

        time_in_force = cls._event_time_in_force(getattr(payload, "time_in_force", None))
        if time_in_force is not None:
            state["time_in_force"] = time_in_force

        quantity = cls._to_decimal(getattr(payload, "quantity", None))
        filled_qty = cls._to_decimal(getattr(payload, "filled_qty", None))
        leaves_qty = cls._to_decimal(getattr(payload, "leaves_qty", None))
        if quantity is None and filled_qty is not None and leaves_qty is not None:
            quantity = filled_qty + leaves_qty
        if quantity is not None:
            state["quantity"] = quantity
        if filled_qty is not None:
            state["filled_qty"] = filled_qty
        if leaves_qty is not None:
            state["leaves_qty"] = leaves_qty

        price = getattr(payload, "price", None)
        if price is not None:
            state["price"] = str(price)

        trigger_price = getattr(payload, "trigger_price", None)
        if trigger_price is not None:
            state["trigger_price"] = str(trigger_price)

        avg_price = cls._to_decimal(getattr(payload, "avg_price", None))
        if avg_price is not None:
            state["avg_px"] = avg_price

        venue_order_id = getattr(payload, "venue_order_id", None)
        if venue_order_id:
            state["venue_order_id"] = VenueOrderId(venue_order_id)

    @staticmethod
    def _is_stale_order_event(state: dict, ts_event: int) -> bool:
        return ts_event < int(state.get("ts_last", 0) or 0)

    @classmethod
    def _fill_event_key(cls, client_order_id: str, filled) -> tuple[str, str, str, int, str, str]:
        return (
            getattr(filled, "trade_id", None) or "",
            client_order_id,
            filled.venue_order_id,
            int(filled.ts_event),
            cls._decimal_str(filled.fill_price),
            cls._decimal_str(filled.fill_qty),
        )

    def _latest_execution_ts_ns(self) -> int:
        latest_order = max((int(state.get("ts_last", 0) or 0) for state in self._orders.values()), default=0)
        latest_fill = max((int(fill.get("ts_event", 0) or 0) for fill in self._fills), default=0)
        return max(latest_order, latest_fill)

    def _execution_replay_window(self) -> tuple[int, int]:
        now_ns = self._clock.timestamp_ns()
        latest_ns = self._latest_execution_ts_ns()
        if latest_ns > 0:
            start_sec = max(0, int(latest_ns // 1_000_000_000) - 1)
        else:
            start_sec = max(0, int(now_ns // 1_000_000_000) - self._config.execution_replay_lookback_secs)
        finish_sec = max(start_sec, int(now_ns // 1_000_000_000) + 1)
        return start_sec, finish_sec

    def _clear_open_orders_for_reconcile(self) -> None:
        stale_ids = [
            client_order_id
            for client_order_id, state in self._orders.items()
            if self._is_open_order_status(self._order_status(state.get("status")))
        ]
        for client_order_id in stale_ids:
            self._orders.pop(client_order_id, None)

    async def _reconcile_execution_state(self) -> None:
        if not self._client:
            raise RuntimeError("Execution client not connected")

        start_sec, finish_sec = self._execution_replay_window()
        self._log.debug(
            f"Replaying execution recovery window start={start_sec} finish={finish_sec}"
        )
        self._clear_open_orders_for_reconcile()
        await self._client.replay_executions(start_sec, finish_sec)
        await self._client.query_orders()
        await self._warn_unrecoverable_native_brackets()

    async def _unrecoverable_native_bracket_parent_venue_ids(self) -> list[str]:
        if not self._client:
            raise RuntimeError("Execution client not connected")

        if not hasattr(self._client, "show_brackets") or not hasattr(self._client, "show_bracket_stops"):
            return []

        try:
            bracket_rows = await self._client.show_brackets()
            stop_rows = await self._client.show_bracket_stops()
        except Exception as exc:
            self._log.debug(f"Failed to query venue-native bracket metadata during reconcile: {exc}")
            return []

        active_parent_venue_ids = set()
        for row in list(bracket_rows or []) + list(stop_rows or []):
            if not isinstance(row, dict):
                continue
            basket_id = row.get("basket_id")
            if basket_id:
                active_parent_venue_ids.add(basket_id)

        return sorted(
            basket_id
            for basket_id in active_parent_venue_ids
            if basket_id not in self._native_brackets_by_parent_venue_id
        )

    async def _warn_unrecoverable_native_brackets(self) -> None:
        unresolved = await self._unrecoverable_native_bracket_parent_venue_ids()
        if not unresolved:
            return

        self._log.warning(
            "Active native venue brackets could not be fully reconstructed for parent basket IDs "
            f"{unresolved}. Child client_order_id attribution remains unavailable unless the "
            "adapter created and persisted those brackets locally."
        )

    async def _submit_nautilus_order(self, order) -> None:
        if not self._client:
            raise RuntimeError("Execution client not connected")

        instrument_id = order.instrument_id
        price = getattr(order, "price", None)
        trigger_price = getattr(order, "trigger_price", None)
        trailing_offset = getattr(order, "trailing_offset", None)

        await self._client.submit_order(
            symbol=instrument_id.symbol,
            exchange=instrument_id.venue.value,
            side=self._to_rithmic_side(order.side),
            order_type=self._to_rithmic_order_type(order.order_type),
            quantity=int(float(order.quantity)),
            client_order_id=order.client_order_id.value,
            price=float(price) if price is not None else None,
            stop_price=float(trigger_price) if trigger_price is not None else None,
            time_in_force=self._to_rithmic_tif(order.time_in_force),
            trailing_stop_ticks=int(float(trailing_offset)) if trailing_offset is not None else None,
        )
        venue_order_id = self._tracked_venue_order_id(order.client_order_id.value)
        self._seed_order_state(order, status=OrderStatus.SUBMITTED, venue_order_id=venue_order_id)

    async def _submit_native_bracket_order_list(self, order_list) -> None:
        if not self._client:
            raise RuntimeError("Execution client not connected")

        orders = list(order_list.orders)
        if len(orders) != 3:
            raise ValueError("Native Rithmic bracket submission requires exactly 3 orders")

        parents = [order for order in orders if getattr(order, "parent_order_id", None) is None]
        children = [order for order in orders if getattr(order, "parent_order_id", None) is not None]
        if len(parents) != 1 or len(children) != 2:
            raise ValueError("Bracket order list must contain one parent entry and two child exit orders")

        parent = parents[0]
        if any(getattr(order, "parent_order_id", None) != parent.client_order_id for order in children):
            raise ValueError("Bracket child orders must reference the parent client_order_id")
        child_map = {order.order_type: order for order in children}
        stop_order = child_map.get(OrderType.STOP_MARKET)
        target_order = child_map.get(OrderType.LIMIT)
        if stop_order is None or target_order is None:
            raise ValueError(
                "Native Rithmic brackets require a LIMIT take-profit child and STOP_MARKET stop-loss child"
            )

        if parent.order_type != OrderType.LIMIT:
            raise ValueError(
                "Native Rithmic brackets currently support only LIMIT entry orders because venue brackets require tick offsets"
            )

        parent_side = parent.side
        expected_exit_side = OrderSide.SELL if parent_side == OrderSide.BUY else OrderSide.BUY
        if stop_order.side != expected_exit_side or target_order.side != expected_exit_side:
            raise ValueError("Bracket exit orders must be opposite the parent entry side")

        parent_quantity = self._to_decimal(parent.quantity)
        stop_quantity = self._to_decimal(stop_order.quantity)
        target_quantity = self._to_decimal(target_order.quantity)
        if parent_quantity is None or stop_quantity != parent_quantity or target_quantity != parent_quantity:
            raise ValueError("Bracket child quantities must match the parent entry quantity")

        if stop_order.time_in_force != parent.time_in_force or target_order.time_in_force != parent.time_in_force:
            raise ValueError("Native Rithmic brackets require entry and child orders to share the same time_in_force")

        if getattr(stop_order, "post_only", False) or getattr(target_order, "post_only", False):
            raise ValueError("Native Rithmic brackets do not support post_only child orders")

        instrument = self._find_instrument(parent.instrument_id)
        if instrument is None:
            raise RuntimeError(
                f"Instrument {parent.instrument_id} is not available in cache/provider for bracket tick conversion"
            )

        entry_price = getattr(parent, "price", None)
        target_price = getattr(target_order, "price", None)
        stop_trigger_price = getattr(stop_order, "trigger_price", None)
        if entry_price is None or target_price is None or stop_trigger_price is None:
            raise ValueError("Native Rithmic brackets require explicit entry, target, and stop prices")

        if parent_side == OrderSide.BUY:
            profit_ticks = self._tick_distance(entry_price, target_price, instrument.price_increment, "Take-profit")
            stop_ticks = self._tick_distance(stop_trigger_price, entry_price, instrument.price_increment, "Stop-loss")
        else:
            profit_ticks = self._tick_distance(target_price, entry_price, instrument.price_increment, "Take-profit")
            stop_ticks = self._tick_distance(entry_price, stop_trigger_price, instrument.price_increment, "Stop-loss")

        await self._client.submit_bracket_order(
            symbol=parent.instrument_id.symbol,
            exchange=parent.instrument_id.venue.value,
            side=self._to_rithmic_side(parent.side),
            order_type=self._to_rithmic_order_type(parent.order_type),
            quantity=int(float(parent.quantity)),
            client_order_id=parent.client_order_id.value,
            profit_ticks=profit_ticks,
            stop_ticks=stop_ticks,
            price=float(entry_price),
            time_in_force=self._to_rithmic_tif(parent.time_in_force),
        )

        parent_venue_order_id = self._tracked_venue_order_id(parent.client_order_id.value)
        self._seed_order_state(parent, status=OrderStatus.SUBMITTED, venue_order_id=parent_venue_order_id)
        self._seed_order_state(stop_order, status=OrderStatus.INITIALIZED)
        self._seed_order_state(target_order, status=OrderStatus.INITIALIZED)
        self._register_native_bracket(
            parent,
            stop_order,
            target_order,
            parent_venue_order_id.value if parent_venue_order_id is not None else None,
        )

    async def _submit_native_oco_order_list(self, order_list) -> None:
        if not self._client:
            raise RuntimeError("Execution client not connected")

        orders = list(order_list.orders)
        if len(orders) != 2:
            raise ValueError("Native Rithmic OCO submission requires exactly 2 orders")

        first_order, second_order = orders
        linked_first = {linked.value for linked in getattr(first_order, "linked_order_ids", None) or []}
        linked_second = {linked.value for linked in getattr(second_order, "linked_order_ids", None) or []}
        if (
            getattr(first_order, "contingency_type", None) != ContingencyType.OCO
            or getattr(second_order, "contingency_type", None) != ContingencyType.OCO
            or second_order.client_order_id.value not in linked_first
            or first_order.client_order_id.value not in linked_second
            or getattr(first_order, "parent_order_id", None) is not None
            or getattr(second_order, "parent_order_id", None) is not None
        ):
            raise ValueError("Order list is not a supported native OCO pair")

        await self._client.submit_oco_order(
            leg1_symbol=first_order.instrument_id.symbol,
            leg1_exchange=first_order.instrument_id.venue.value,
            leg1_side=self._to_rithmic_side(first_order.side),
            leg1_order_type=self._to_rithmic_order_type(first_order.order_type),
            leg1_quantity=int(float(first_order.quantity)),
            leg1_client_order_id=first_order.client_order_id.value,
            leg1_price=(
                float(getattr(first_order, "price", None))
                if getattr(first_order, "price", None) is not None
                else None
            ),
            leg1_stop_price=(
                float(getattr(first_order, "trigger_price", None))
                if getattr(first_order, "trigger_price", None) is not None
                else None
            ),
            leg1_time_in_force=self._to_rithmic_tif(first_order.time_in_force),
            leg2_symbol=second_order.instrument_id.symbol,
            leg2_exchange=second_order.instrument_id.venue.value,
            leg2_side=self._to_rithmic_side(second_order.side),
            leg2_order_type=self._to_rithmic_order_type(second_order.order_type),
            leg2_quantity=int(float(second_order.quantity)),
            leg2_client_order_id=second_order.client_order_id.value,
            leg2_price=(
                float(getattr(second_order, "price", None))
                if getattr(second_order, "price", None) is not None
                else None
            ),
            leg2_stop_price=(
                float(getattr(second_order, "trigger_price", None))
                if getattr(second_order, "trigger_price", None) is not None
                else None
            ),
            leg2_time_in_force=self._to_rithmic_tif(second_order.time_in_force),
        )

        self._seed_order_state(
            first_order,
            status=OrderStatus.SUBMITTED,
            venue_order_id=self._tracked_venue_order_id(first_order.client_order_id.value),
        )
        self._seed_order_state(
            second_order,
            status=OrderStatus.SUBMITTED,
            venue_order_id=self._tracked_venue_order_id(second_order.client_order_id.value),
        )

    async def _connect(self) -> None:
        """Connect to the Rithmic order plant."""
        self._balances.clear()
        self._positions.clear()
        self._primary_balance_event.clear()
        self._accessible_accounts = []
        self._gateway = RithmicGateway(
            environment=self._config.environment,
            username=self._config.username,
            password=self._config.password,
            system_name=self._config.system_name,
            app_name=self._config.app_name,
            app_version=self._config.app_version,
            fcm_id=self._config.fcm_id or "",
            ib_id=self._config.ib_id or "",
            account_id=self._config.account_id,
            enable_ticker=False,
            enable_order=True,
            enable_pnl=True,
            enable_history=False,
        )

        await self._gateway.connect()

        try:
            accounts = await self._gateway.list_accounts()
        except Exception:
            self._log.exception("Failed to load accessible accounts from the order plant")
            accounts = []

        self._accessible_accounts = sorted(set(accounts))
        if self._accessible_accounts and self._config.account_id not in self._accessible_accounts:
            raise RuntimeError(
                "Configured account "
                f"{self._config.account_id!r} is not available to these credentials "
                f"{self._accessible_accounts!r}"
            )

        self._client = RithmicExecutionClient(self._gateway, self._config.account_id)

        # Surface events to Nautilus callbacks via the existing Py callback
        self._client.set_execution_callback(self._on_execution_event)
        await self._client.start_event_loop()

        # Pipe PnL/position events if available
        try:
            self._gateway.start_pnl_loop(self._on_pnl_event)
            self._pnl_live = True
        except RuntimeError:
            self._pnl_live = False
            self._log.debug("PnL loop not started (receiver unavailable)")

        if not self._pnl_live:
            raise RuntimeError("PnL loop unavailable; cannot register Nautilus account state")

        await self._gateway.request_pnl_snapshot()
        await self._wait_for_primary_balance()
        self._refresh_account_state()
        await self._await_account_registered()
        await self._reconcile_execution_state()

        self._log.info("Connected to Rithmic order plant")

    async def _disconnect(self) -> None:
        """Disconnect from the Rithmic order plant."""
        if self._client:
            self._client.stop_event_loop()
            self._client.clear_execution_callback()
            self._client = None

        if self._gateway:
            self._gateway.stop_pnl_loop()
            self._pnl_live = False
            await self._gateway.disconnect()
            self._gateway = None

        self._primary_balance_event.clear()
        self._accessible_accounts = []

        self._log.info("Disconnected from Rithmic order plant")

    def _on_execution_event(self, event) -> None:
        """Handle execution events from Rust."""
        try:
            if event.is_error():
                error = event.as_error()
                self._log.error(f"Execution error: {error}")
                return

            if event.is_rejected():
                rejected = event.as_rejected()
                client_order_id = self._resolve_event_client_order_id(rejected)
                self._log.warning(
                    f"Order rejected: {client_order_id} reason={rejected.reason}"
                )
                state = self._orders.setdefault(client_order_id, {})
                self._apply_native_bracket_order_seed(client_order_id, state)
                state.setdefault("client_order_id", ClientOrderId(client_order_id))
                state.setdefault("ts_init", rejected.ts_event)
                if self._is_stale_order_event(state, rejected.ts_event):
                    return
                self._apply_execution_context(state, rejected)
                state["status"] = OrderStatus.REJECTED
                state["cancel_reason"] = rejected.reason
                state["ts_last"] = rejected.ts_event
                if client_order_id in self._native_brackets and self._native_bracket_role(getattr(rejected, "bracket_type", None)) is None:
                    self._propagate_native_bracket_parent_terminal(
                        client_order_id,
                        status=OrderStatus.REJECTED,
                        ts_event=rejected.ts_event,
                        reason=rejected.reason,
                    )
                self._maybe_retire_native_bracket_by_client_order_id(client_order_id)
                return

            if event.is_submitted():
                submitted = event.as_submitted()
                client_order_id = self._resolve_event_client_order_id(submitted)
                self._log.debug(
                    f"Order submitted: {client_order_id} venue_id={submitted.venue_order_id}"
                )
                state = self._orders.setdefault(client_order_id, {})
                self._apply_native_bracket_order_seed(client_order_id, state)
                state.setdefault("client_order_id", ClientOrderId(client_order_id))
                state.setdefault("ts_init", submitted.ts_event)
                if self._is_stale_order_event(state, submitted.ts_event):
                    return
                self._apply_execution_context(state, submitted)
                state["status"] = OrderStatus.SUBMITTED
                state["ts_last"] = submitted.ts_event
                self._update_native_bracket_parent_venue_id(client_order_id, submitted.venue_order_id)
                return

            if event.is_accepted():
                accepted = event.as_accepted()
                client_order_id = self._resolve_event_client_order_id(accepted)
                self._log.debug(
                    f"Order accepted: {client_order_id} venue_id={accepted.venue_order_id}"
                )
                state = self._orders.setdefault(client_order_id, {})
                self._apply_native_bracket_order_seed(client_order_id, state)
                state.setdefault("client_order_id", ClientOrderId(client_order_id))
                state.setdefault("ts_init", accepted.ts_event)
                if self._is_stale_order_event(state, accepted.ts_event):
                    return
                self._apply_execution_context(state, accepted)
                state["status"] = OrderStatus.ACCEPTED
                state["ts_accepted"] = accepted.ts_event
                state["ts_last"] = accepted.ts_event
                if self._native_bracket_role(getattr(accepted, "bracket_type", None)) is None:
                    self._update_native_bracket_parent_venue_id(client_order_id, accepted.venue_order_id)
                return

            if event.is_filled():
                filled = event.as_filled()
                client_order_id = self._resolve_event_client_order_id(filled)
                self._log.debug(
                    f"Order filled: {client_order_id} qty={filled.fill_qty} price={filled.fill_price}"
                )
                fill_key = self._fill_event_key(client_order_id, filled)
                if fill_key in self._seen_fill_keys:
                    self._log.debug(
                        f"Skipping replayed fill {fill_key[0] or filled.venue_order_id} "
                        f"for order {client_order_id}"
                    )
                    return
                self._seen_fill_keys.add(fill_key)

                state = self._orders.setdefault(client_order_id, {})
                self._apply_native_bracket_order_seed(client_order_id, state)
                state.setdefault("client_order_id", ClientOrderId(client_order_id))
                state.setdefault("ts_init", filled.ts_event)
                stale = self._is_stale_order_event(state, filled.ts_event)
                self._apply_execution_context(state, filled)
                filled_qty = (state.get("filled_qty") or Decimal("0")) + Decimal(str(filled.fill_qty))
                leaves_qty = Decimal(str(filled.leaves_qty))
                if not stale:
                    state.update(
                        {
                            "status": OrderStatus.PARTIALLY_FILLED if filled.leaves_qty > 0 else OrderStatus.FILLED,
                            "filled_qty": filled_qty,
                            "leaves_qty": leaves_qty,
                            "avg_px": Decimal(str(filled.fill_price)),
                            "venue_order_id": VenueOrderId(filled.venue_order_id),
                            "ts_last": filled.ts_event,
                        }
                    )
                instrument_id = state.get("instrument_id")
                side = state.get("order_side")
                self._fills.append(
                    {
                        "client_order_id": client_order_id,
                        "venue_order_id": filled.venue_order_id,
                        "price": Decimal(str(filled.fill_price)),
                        "qty": Decimal(str(filled.fill_qty)),
                        "leaves": leaves_qty,
                        "commission": Decimal(str(filled.commission)),
                        "ts_event": filled.ts_event,
                        "ts_init": self._clock.timestamp_ns(),
                        "trade_id": getattr(filled, "trade_id", None),
                        "instrument_id": instrument_id,
                        "side": side,
                        "currency": getattr(filled, "currency", None)
                        or self._balances.get(self._config.account_id, {}).get("currency", "USD"),
                    }
                )

                # Derive positions locally only if PnL events are not available.
                if instrument_id and side and not self._pnl_live and not stale:
                    account_id = self._require_account_id()
                    key = f"{account_id.value}:{instrument_id.value}"
                    pos = self._positions.get(key, {
                        "account_id": account_id.value,
                        "instrument_id": instrument_id,
                        "quantity": Decimal("0"),
                        "avg_price": Decimal("0"),
                        "currency": "USD",
                    })

                    signed_qty = Decimal(str(filled.fill_qty)) if side == OrderSide.BUY else -Decimal(str(filled.fill_qty))
                    new_qty = pos["quantity"] + signed_qty

                    # Weighted average price if position remains open and same direction
                    if new_qty != 0:
                        prev_notional = pos["avg_price"] * pos["quantity"]
                        new_notional = prev_notional + (Decimal(str(filled.fill_price)) * signed_qty)
                        pos["avg_price"] = new_notional / new_qty
                    else:
                        pos["avg_price"] = Decimal("0")

                    pos["quantity"] = new_qty
                    pos.setdefault("currency", "USD")
                    self._positions[key] = pos
                self._maybe_retire_native_bracket_by_client_order_id(client_order_id)
                return

            if event.is_cancelled():
                cancelled = event.as_cancelled()
                client_order_id = self._resolve_event_client_order_id(cancelled)
                self._log.debug(
                    f"Order cancelled: {client_order_id} venue_id={cancelled.venue_order_id}"
                )
                state = self._orders.setdefault(client_order_id, {})
                self._apply_native_bracket_order_seed(client_order_id, state)
                state.setdefault("client_order_id", ClientOrderId(client_order_id))
                state.setdefault("ts_init", cancelled.ts_event)
                if self._is_stale_order_event(state, cancelled.ts_event):
                    return
                self._apply_execution_context(state, cancelled)
                state.update(
                    {
                        "status": OrderStatus.CANCELED,
                        "venue_order_id": VenueOrderId(cancelled.venue_order_id),
                        "leaves_qty": Decimal("0"),
                        "ts_last": cancelled.ts_event,
                    }
                )
                if client_order_id in self._native_brackets and self._native_bracket_role(getattr(cancelled, "bracket_type", None)) is None:
                    self._propagate_native_bracket_parent_terminal(
                        client_order_id,
                        status=OrderStatus.CANCELED,
                        ts_event=cancelled.ts_event,
                    )
                self._maybe_retire_native_bracket_by_client_order_id(client_order_id)
                return

            if event.is_modified():
                modified = event.as_modified()
                client_order_id = self._resolve_event_client_order_id(modified)
                self._log.debug(
                    f"Order modified: {client_order_id} price={modified.new_price} qty={modified.new_qty}"
                )
                state = self._orders.setdefault(client_order_id, {})
                self._apply_native_bracket_order_seed(client_order_id, state)
                state.setdefault("client_order_id", ClientOrderId(client_order_id))
                state.setdefault("ts_init", modified.ts_event)
                if self._is_stale_order_event(state, modified.ts_event):
                    return
                self._apply_execution_context(state, modified)
                if modified.new_price is not None:
                    state["price"] = str(modified.new_price)
                if modified.new_qty is not None:
                    state["leaves_qty"] = Decimal(str(modified.new_qty))
                state["status"] = OrderStatus.PENDING_UPDATE
                state["ts_last"] = modified.ts_event
        except _HANDLER_EXCEPTIONS as exc:
            self._log.error(f"Error handling execution event: {exc}")

    def _on_pnl_event(self, event) -> None:
        """Handle PnL/account/position events emitted from the gateway."""
        try:
            # Account-level balances
            if isinstance(event, AccountEvent) or (
                hasattr(event, "currency") and hasattr(event, "total")
            ):
                account_id = getattr(event, "account_id", None) or self._config.account_id
                currency = getattr(event, "currency", None) or "USD"
                self._balances[account_id] = {
                    "currency": currency,
                    "total": self._to_decimal(getattr(event, "total", 0.0)) or Decimal("0"),
                    "available": self._to_decimal(getattr(event, "available", 0.0)) or Decimal("0"),
                    "locked": self._to_decimal(getattr(event, "locked", 0.0)) or Decimal("0"),
                    "unrealized_pnl": self._to_decimal(getattr(event, "unrealized_pnl", 0.0)) or Decimal("0"),
                    "realized_pnl": self._to_decimal(getattr(event, "realized_pnl", 0.0)) or Decimal("0"),
                }
                if account_id == self._config.account_id:
                    ts_event = getattr(event, "ts_event", self._clock.timestamp_ns())
                    self._signal_primary_balance()
                    try:
                        running_loop = asyncio.get_running_loop()
                    except RuntimeError:
                        running_loop = None

                    if running_loop is self._loop:
                        self._refresh_account_state(account_id, ts_event)
                    else:
                        self._loop.call_soon_threadsafe(
                            self._refresh_account_state,
                            account_id,
                            ts_event,
                        )
                return

            # Position-level updates
            if isinstance(event, PositionEvent) or (
                hasattr(event, "symbol") and hasattr(event, "quantity")
            ):
                account_id = getattr(event, "account_id", None) or self._config.account_id
                symbol = getattr(event, "symbol", None)
                if not symbol:
                    return

                instrument_id = InstrumentId.from_str(f"{symbol}.{RITHMIC_VENUE.value}")
                exchange = getattr(event, "exchange", None)
                account = self._require_account_id()
                key = f"{account.value}:{instrument_id.value}"
                currency = self._balances.get(account_id, {}).get("currency", "USD")
                quantity = self._to_decimal(getattr(event, "quantity", 0.0)) or Decimal("0")

                if quantity == 0:
                    self._positions.pop(key, None)
                    return

                self._positions[key] = {
                    "account_id": account.value,
                    "instrument_id": instrument_id,
                    "exchange": exchange,
                    "quantity": quantity,
                    "avg_price": self._to_decimal(getattr(event, "avg_price", 0.0)) or Decimal("0"),
                    "unrealized_pnl": self._to_decimal(getattr(event, "unrealized_pnl", 0.0)) or Decimal("0"),
                    "realized_pnl": self._to_decimal(getattr(event, "realized_pnl", 0.0)) or Decimal("0"),
                    "currency": currency,
                    "ts_event": getattr(event, "ts_event", 0),
                }
                return
        except _HANDLER_EXCEPTIONS as exc:
            self._log.error(f"Error handling PnL event: {exc}")

    async def _submit_order(self, command: SubmitOrder) -> None:
        """
        Submit an order.

        Parameters
        ----------
        command : SubmitOrder
            The command to submit the order.
        """
        self._assert_order_account_scope(command.order)
        await self._submit_nautilus_order(command.order)

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        """
        Submit an order list (bracket orders).

        Parameters
        ----------
        command : SubmitOrderList
            The command to submit the order list.
        """
        for order in command.order_list.orders:
            self._assert_order_account_scope(order)

        orders = list(command.order_list.orders)
        if len(orders) == 3:
            await self._submit_native_bracket_order_list(command.order_list)
            return

        if len(orders) == 2:
            await self._submit_native_oco_order_list(command.order_list)
            return

        raise RuntimeError(
            "Rithmic only supports native bracket (3-leg) or OCO (2-leg) order lists; "
            "sequential SubmitOrderList fallback has been removed"
        )

    async def _modify_order(self, command: ModifyOrder) -> None:
        """
        Modify an existing order.

        Parameters
        ----------
        command : ModifyOrder
            The command to modify the order.
        """
        if not self._client:
            raise RuntimeError("Execution client not connected")

        state = self._find_order_state(
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
        )
        venue_order_id = command.venue_order_id or (state.get("venue_order_id") if state else None)
        if venue_order_id is None:
            raise RuntimeError("Cannot modify order without venue_order_id")

        await self._client.modify_order(
            venue_order_id=venue_order_id.value,
            symbol=command.instrument_id.symbol,
            exchange=command.instrument_id.venue.value,
            new_qty=int(float(command.quantity)) if command.quantity is not None else None,
            new_price=float(command.price) if command.price is not None else None,
            order_type=None,  # keep existing unless price type changes
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        """
        Cancel an order.

        Parameters
        ----------
        command : CancelOrder
            The command to cancel the order.
        """
        if not self._client:
            raise RuntimeError("Execution client not connected")

        state = self._find_order_state(
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
        )
        venue_order_id = command.venue_order_id or (state.get("venue_order_id") if state else None)
        if venue_order_id is None:
            raise RuntimeError("Cannot cancel order without venue_order_id")

        await self._client.cancel_order(venue_order_id.value)

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        """
        Cancel all orders.

        Parameters
        ----------
        command : CancelAllOrders
            The command to cancel all orders.
        """
        if not self._client:
            raise RuntimeError("Execution client not connected")

        await self._client.cancel_all_orders()

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        """
        Batch cancel orders.

        Parameters
        ----------
        command : BatchCancelOrders
            The command to batch cancel orders.
        """
        if not self._client:
            raise RuntimeError("Execution client not connected")

        ids = [
            cancel.venue_order_id.value
            for cancel in command.cancels
            if cancel.venue_order_id is not None
        ]
        await self._client.batch_cancel_orders(ids)

    async def generate_order_status_report(
        self,
        command: "GenerateOrderStatusReport",
    ) -> Optional["OrderStatusReport"]:
        """
        Generate an order status report for the given command.

        Parameters
        ----------
        command : GenerateOrderStatusReport
            The command for the report.

        Returns
        -------
        OrderStatusReport or None
        """
        state = self._find_order_state(
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
        )
        if state is None:
            return None

        instrument_id = state.get("instrument_id") or command.instrument_id
        venue_order_id = state.get("venue_order_id") or command.venue_order_id
        if instrument_id is None:
            return None
        if venue_order_id is None:
            return None

        return OrderStatusReport(
            account_id=self._require_account_id(),
            instrument_id=instrument_id,
            venue_order_id=venue_order_id,
            order_side=state.get("order_side", OrderSide.NO_ORDER_SIDE),
            order_type=state.get("order_type", OrderType.MARKET),
            time_in_force=state.get("time_in_force", TimeInForce.DAY),
            order_status=self._order_status(state.get("status")),
            quantity=self._to_quantity(state.get("quantity", 0)),
            filled_qty=self._to_quantity(state.get("filled_qty", 0)),
            report_id=UUID4(),
            ts_accepted=state.get("ts_accepted", 0),
            ts_last=state.get("ts_last", state.get("ts_init", 0)),
            ts_init=self._clock.timestamp_ns(),
            client_order_id=state.get("client_order_id", command.client_order_id),
            order_list_id=state.get("order_list_id"),
            linked_order_ids=state.get("linked_order_ids"),
            parent_order_id=state.get("parent_order_id"),
            contingency_type=state.get("contingency_type", ContingencyType.NO_CONTINGENCY),
            expire_time=state.get("expire_time"),
            price=self._to_price(state.get("price")),
            trigger_price=self._to_price(state.get("trigger_price")),
            trigger_type=state.get("trigger_type", TriggerType.NO_TRIGGER),
            avg_px=state.get("avg_px"),
            display_qty=state.get("display_qty"),
            post_only=state.get("post_only", False),
            reduce_only=state.get("reduce_only", False),
            cancel_reason=state.get("cancel_reason"),
        )

    async def generate_order_status_reports(
        self,
        command: "GenerateOrderStatusReports",
    ) -> list["OrderStatusReport"]:
        """
        Generate order status reports for the given command.

        Parameters
        ----------
        command : GenerateOrderStatusReports
            The command for the reports.

        Returns
        -------
        list[OrderStatusReport]
        """
        reports: list[OrderStatusReport] = []
        start_ns = self._datetime_to_ns(command.start)
        end_ns = self._datetime_to_ns(command.end)

        for state in sorted(self._orders.values(), key=lambda s: s.get("ts_last", 0)):
            instrument_id = state.get("instrument_id")
            venue_order_id = state.get("venue_order_id")
            if instrument_id is None or venue_order_id is None:
                continue
            if command.instrument_id is not None and instrument_id != command.instrument_id:
                continue
            status = self._order_status(state.get("status"))
            if command.open_only and not self._is_open_order_status(status):
                continue
            ts_last = state.get("ts_last", 0)
            if start_ns is not None and ts_last < start_ns:
                continue
            if end_ns is not None and ts_last > end_ns:
                continue

            reports.append(
                OrderStatusReport(
                    account_id=self._require_account_id(),
                    instrument_id=instrument_id,
                    venue_order_id=venue_order_id,
                    order_side=state.get("order_side", OrderSide.NO_ORDER_SIDE),
                    order_type=state.get("order_type", OrderType.MARKET),
                    time_in_force=state.get("time_in_force", TimeInForce.DAY),
                    order_status=status,
                    quantity=self._to_quantity(state.get("quantity", 0)),
                    filled_qty=self._to_quantity(state.get("filled_qty", 0)),
                    report_id=UUID4(),
                    ts_accepted=state.get("ts_accepted", 0),
                    ts_last=ts_last,
                    ts_init=self._clock.timestamp_ns(),
                    client_order_id=state.get("client_order_id"),
                    order_list_id=state.get("order_list_id"),
                    linked_order_ids=state.get("linked_order_ids"),
                    parent_order_id=state.get("parent_order_id"),
                    contingency_type=state.get("contingency_type", ContingencyType.NO_CONTINGENCY),
                    expire_time=state.get("expire_time"),
                    price=self._to_price(state.get("price")),
                    trigger_price=self._to_price(state.get("trigger_price")),
                    trigger_type=state.get("trigger_type", TriggerType.NO_TRIGGER),
                    avg_px=state.get("avg_px"),
                    display_qty=state.get("display_qty"),
                    post_only=state.get("post_only", False),
                    reduce_only=state.get("reduce_only", False),
                    cancel_reason=state.get("cancel_reason"),
                )
            )

        return reports

    async def generate_fill_reports(
        self,
        command: "GenerateFillReports",
    ) -> list["FillReport"]:
        """
        Generate fill reports for the given command.

        Parameters
        ----------
        command : GenerateFillReports
            The command for the reports.

        Returns
        -------
        list[FillReport]
        """
        reports: list[FillReport] = []
        start_ns = self._datetime_to_ns(command.start)
        end_ns = self._datetime_to_ns(command.end)

        fills = sorted(self._fills, key=lambda f: f.get("ts_event", 0))
        for fill in fills:
            instrument_id = fill.get("instrument_id")
            venue_order_id = fill.get("venue_order_id")
            if instrument_id is None or venue_order_id is None:
                continue
            if command.instrument_id is not None and instrument_id != command.instrument_id:
                continue
            if command.venue_order_id is not None and venue_order_id != command.venue_order_id.value:
                continue
            ts_event = fill.get("ts_event", 0)
            if start_ns is not None and ts_event < start_ns:
                continue
            if end_ns is not None and ts_event > end_ns:
                continue

            currency = self._to_currency(fill.get("currency", "USD"))
            side = fill.get("side", OrderSide.BUY)
            trade_id = fill.get("trade_id")

            reports.append(
                FillReport(
                    account_id=self._require_account_id(),
                    instrument_id=instrument_id,
                    venue_order_id=VenueOrderId(venue_order_id),
                    trade_id=TradeId(str(trade_id or UUID4())),
                    order_side=side,
                    last_qty=self._to_quantity(fill["qty"]),
                    last_px=self._to_price(fill["price"]),
                    commission=Money(float(fill.get("commission", 0.0)), currency),
                    liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
                    report_id=UUID4(),
                    ts_event=ts_event,
                    ts_init=fill.get("ts_init", self._clock.timestamp_ns()),
                    client_order_id=ClientOrderId(fill["client_order_id"]),
                    venue_position_id=None,
                )
            )

        return reports

    async def generate_position_status_reports(
        self,
        command: "GeneratePositionStatusReports",
    ) -> list["PositionStatusReport"]:
        """
        Generate position status reports for the given command.

        Parameters
        ----------
        command : GeneratePositionStatusReports
            The command for the reports.

        Returns
        -------
        list[PositionStatusReport]
        """
        reports: list[PositionStatusReport] = []
        start_ns = self._datetime_to_ns(command.start)
        end_ns = self._datetime_to_ns(command.end)

        for pos in self._positions.values():
            instrument_id = pos.get("instrument_id")
            if instrument_id is None:
                continue
            if command.instrument_id is not None and instrument_id != command.instrument_id:
                continue
            ts_last = pos.get("ts_event", 0)
            if start_ns is not None and ts_last < start_ns:
                continue
            if end_ns is not None and ts_last > end_ns:
                continue
            quantity = self._to_decimal(pos.get("quantity", 0)) or Decimal("0")
            position_side = self._position_side(quantity)
            if position_side == PositionSide.FLAT:
                continue

            reports.append(
                PositionStatusReport(
                    account_id=AccountId(pos.get("account_id", "RITHMIC")),
                    instrument_id=instrument_id,
                    position_side=position_side,
                    quantity=self._to_quantity(abs(quantity)),
                    report_id=UUID4(),
                    ts_last=ts_last,
                    ts_init=self._clock.timestamp_ns(),
                    avg_px_open=self._to_decimal(pos.get("avg_price")),
                )
            )

        return reports

    async def generate_mass_status(
        self,
        lookback_mins: Optional[int] = None,
    ) -> Optional["ExecutionMassStatus"]:
        """
        Generate an execution mass status report.

        Parameters
        ----------
        lookback_mins : int, optional
            The lookback period in minutes.

        Returns
        -------
        ExecutionMassStatus or None
        """
        return await super().generate_mass_status(lookback_mins)
