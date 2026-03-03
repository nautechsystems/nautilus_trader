from __future__ import annotations

from collections.abc import Iterable
from decimal import Decimal
import json
from typing import Any

try:
    from examples.live.poc.contracts import TOPIC_ALERT
    from examples.live.poc.contracts import TOPIC_BALANCES
    from examples.live.poc.contracts import TOPIC_EVENT
    from examples.live.poc.contracts import TOPIC_FV
    from examples.live.poc.contracts import TOPIC_MARKET_BBO
    from examples.live.poc.contracts import TOPIC_STATE
    from examples.live.poc.contracts import TOPIC_TRADE
    from examples.live.poc.contracts import get_instrument_contract
    from examples.live.poc.contracts import make_fv_coin
    from examples.live.poc.contracts import json_dumps_compact
    _CONTRACT_HELPERS_AVAILABLE = True
except Exception:  # pragma: no cover - test import environments
    TOPIC_STATE = "maker_poc.state"
    TOPIC_EVENT = "maker_poc.event"
    TOPIC_TRADE = "maker_poc.trade"
    TOPIC_ALERT = "maker_poc.alert"
    TOPIC_MARKET_BBO = "maker_poc.market_bbo"
    TOPIC_FV = "maker_poc.fv"
    TOPIC_BALANCES = "maker_poc.balances"
    _CONTRACT_HELPERS_AVAILABLE = False
    json_dumps_compact = None

    def get_instrument_contract(*_args: Any, **_kwargs: Any) -> Any:
        return None

    def make_fv_coin(value: str) -> str:
        parts = value.split("/", maxsplit=1)
        if len(parts) == 2 and parts[0] and parts[1]:
            return f"{parts[0].lower()}/{parts[1].lower()}"
        raise ValueError(f"Invalid symbol: {value!r}")


def _to_json_safe(payload: Any) -> str:
    if json_dumps_compact is None:
        return json.dumps(payload, sort_keys=True, separators=(",", ":"))
    return json_dumps_compact(payload)


def _to_decimal(value: Decimal | float | str) -> Decimal:
    return value if isinstance(value, Decimal) else Decimal(str(value))


def _validate_three_band_input(values: Iterable[object], name: str) -> tuple[object, object, object]:
    parsed = tuple(values)
    if len(parsed) != 3:
        raise ValueError(f"{name}: expected three bands, got {len(parsed)}")
    return parsed  # type: ignore[return-value]


def build_ladder_targets(
    anchor_bid: Decimal | float | str,
    anchor_ask: Decimal | float | str,
    bid_edges: Iterable[Decimal | float | str],
    ask_edges: Iterable[Decimal | float | str],
    distances: Iterable[Decimal | float | str],
    n_orders: Iterable[int],
) -> tuple[list[Decimal], list[Decimal]]:
    """
    Build 3-band ladder prices from anchor bid/ask and offsets.
    """

    bid_edge_1, bid_edge_2, bid_edge_3 = _validate_three_band_input(bid_edges, "bid_edges")
    ask_edge_1, ask_edge_2, ask_edge_3 = _validate_three_band_input(ask_edges, "ask_edges")
    distance_1, distance_2, distance_3 = _validate_three_band_input(distances, "distances")
    n_1, n_2, n_3 = _validate_three_band_input(n_orders, "n_orders")

    bid_edges = (_to_decimal(bid_edge_1), _to_decimal(bid_edge_2), _to_decimal(bid_edge_3))
    ask_edges = (_to_decimal(ask_edge_1), _to_decimal(ask_edge_2), _to_decimal(ask_edge_3))
    distances = (_to_decimal(distance_1), _to_decimal(distance_2), _to_decimal(distance_3))
    n_orders = (int(n_1), int(n_2), int(n_3))

    if any(v < 0 for v in bid_edges + ask_edges):
        raise ValueError("edges must be non-negative")
    if any(v < 0 for v in distances):
        raise ValueError("distances must be non-negative")
    if any(v < 0 for v in n_orders):
        raise ValueError("n_orders must be non-negative")

    anchor_bid_dec = _to_decimal(anchor_bid)
    anchor_ask_dec = _to_decimal(anchor_ask)

    bid_targets: list[Decimal] = []
    ask_targets: list[Decimal] = []

    for band_idx in range(3):
        for level in range(n_orders[band_idx]):
            step = distances[band_idx] * level
            bid_targets.append(anchor_bid_dec - bid_edges[band_idx] - step)
            ask_targets.append(anchor_ask_dec + ask_edges[band_idx] + step)

    return bid_targets, ask_targets


_NAUTILUS_IMPORT_ERROR: ModuleNotFoundError | None = None
try:
    from nautilus_trader.config import NonNegativeFloat
    from nautilus_trader.config import NonNegativeInt
    from nautilus_trader.config import PositiveInt
    from nautilus_trader.config import StrategyConfig
    from nautilus_trader.model.book import OrderBook
    from nautilus_trader.model.data import OrderBookDeltas
    from nautilus_trader.model.enums import BookType
    from nautilus_trader.model.enums import OrderSide
    from nautilus_trader.model.events import OrderFilled
    from nautilus_trader.model.identifiers import InstrumentId
    from nautilus_trader.model.instruments import Instrument
    from nautilus_trader.model.objects import Quantity
    from nautilus_trader.trading.strategy import Strategy
except ModuleNotFoundError as exc:  # pragma: no cover - pure-math test fallback
    _NAUTILUS_IMPORT_ERROR = exc


if _NAUTILUS_IMPORT_ERROR is None:
    class MakerV3SingleLegQuoterConfig(StrategyConfig, frozen=True):
        bybit_instrument_id: InstrumentId
        binance_instrument_id: InstrumentId
        order_qty: Decimal
        external_strategy_id: str = "bybit_binance_plumeusdt_makerv3"
        bot_on: bool = True
        qty: Decimal | None = None
        hedge_qty: NonNegativeFloat = 0.0
        des_qty_global: NonNegativeFloat = 0.0
        max_qty_global: NonNegativeFloat = 20_000.0
        max_skew_bps_global: NonNegativeFloat = 0.0
        des_qty_local: NonNegativeFloat = 0.0
        max_qty_local: NonNegativeFloat = 0.0
        max_skew_bps_local: NonNegativeFloat = 0.0
        linear_offset_bps: NonNegativeFloat = 0.0
        max_age_ms: PositiveInt = 2_000
        bid_edge1: NonNegativeFloat = 0.05
        ask_edge1: NonNegativeFloat = 0.05
        place_edge1: NonNegativeFloat = 2.0
        distance1: NonNegativeFloat = 0.02
        n_orders1: NonNegativeInt = 1
        bid_edge2: NonNegativeFloat = 0.15
        ask_edge2: NonNegativeFloat = 0.15
        place_edge2: NonNegativeFloat = 2.0
        distance2: NonNegativeFloat = 0.04
        n_orders2: NonNegativeInt = 1
        bid_edge3: NonNegativeFloat = 0.35
        ask_edge3: NonNegativeFloat = 0.35
        place_edge3: NonNegativeFloat = 2.0
        distance3: NonNegativeFloat = 0.08
        n_orders3: NonNegativeInt = 1
        bid_edge_hedge: NonNegativeFloat = 0.0
        ask_edge_hedge: NonNegativeFloat = 0.0
        distance_hedge: NonNegativeFloat = 0.0
        n_orders_hedge: NonNegativeInt = 0
        place_edge_hedge: NonNegativeFloat = 2.0
        strategy_take_enabled: bool = False
        bid_edge_take: NonNegativeFloat = 0.0
        ask_edge_take: NonNegativeFloat = 0.0
        take_qty: NonNegativeFloat = 0.0
        take_cooldown: NonNegativeFloat = 0.0
        hedge_reduce_only: bool = True
        hedge_touch_at_max_qty: bool = False
        quote_fail_critical_after_count: NonNegativeInt = 3
        quote_fail_critical_after_s: NonNegativeFloat = 60.0
        maker_price_anchor: str = "reference_leg"

        @property
        def active_order_qty(self) -> Decimal:
            return self.qty if self.qty is not None else self.order_qty


    class MakerV3SingleLegQuoter(Strategy):
        INTERNAL_REQUOTE_THROTTLE_MS = 150
        BALANCES_PUBLISH_INTERVAL_MS = 10_000

        def __init__(self, config: MakerV3SingleLegQuoterConfig) -> None:
            super().__init__(config)
            self._bybit_instrument: Instrument | None = None
            self._order_qty: Quantity | None = None
            self._price_precision: int = 8
            self._books: dict[InstrumentId, OrderBook] = {}
            self._last_bbo: dict[InstrumentId, tuple[str, str] | None] = {}
            self._last_requote_ns = 0
            self._last_fv: Decimal | None = None
            self._last_fv_snapshot_ts_ns = 0
            self._last_state_ns = 0
            self._last_balances_ns = 0
            self._external_strategy_id = (
                self.config.external_strategy_id.strip()
                if self.config.external_strategy_id
                else "bybit_binance_plumeusdt_makerv3"
            )

        def on_start(self) -> None:
            instrument_id = self.config.bybit_instrument_id
            self._bybit_instrument = self.cache.instrument(instrument_id)
            if self._bybit_instrument is None:
                self._publish_alert(f"Could not find instrument for {instrument_id}")
                self.stop()
                return

            instrument_text = str(instrument_id)
            if ".BYBIT" not in instrument_text or "-LINEAR" not in instrument_text:
                self._publish_alert(
                    f"Execution leg must be Bybit linear only, got {instrument_text}",
                    level="error",
                )
                self.stop()
                return

            try:
                self._order_qty = self._bybit_instrument.make_qty(self.config.active_order_qty)
            except ValueError:
                self._publish_alert(
                    f"Invalid order quantity configured for {instrument_id}",
                    level="error",
                )
                self.stop()
                return
            self._price_precision = self._bybit_instrument.price_precision

            self._books = {
                self.config.bybit_instrument_id: OrderBook(
                    instrument_id=self.config.bybit_instrument_id,
                    book_type=BookType.L2_MBP,
                ),
                self.config.binance_instrument_id: OrderBook(
                    instrument_id=self.config.binance_instrument_id,
                    book_type=BookType.L2_MBP,
                ),
            }
            self._last_bbo = {key: None for key in self._books}

            self.subscribe_order_book_deltas(
                instrument_id=self.config.bybit_instrument_id,
                book_type=BookType.L2_MBP,
            )
            self.subscribe_order_book_deltas(
                instrument_id=self.config.binance_instrument_id,
                book_type=BookType.L2_MBP,
            )

            self._publish_event("started")
            self._publish_balances()
            self._publish_state("on_start")

        def on_stop(self) -> None:
            self._cancel_managed_quotes("on_stop")
            self.unsubscribe_order_book_deltas(
                instrument_id=self.config.bybit_instrument_id,
                book_type=BookType.L2_MBP,
            )
            self.unsubscribe_order_book_deltas(
                instrument_id=self.config.binance_instrument_id,
                book_type=BookType.L2_MBP,
            )
            self._publish_state("on_stop")

        def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
            book = self._books.get(deltas.instrument_id)
            if book is None:
                return

            book.apply_deltas(deltas)
            bid = book.best_bid_price()
            ask = book.best_ask_price()
            if bid is None or ask is None:
                return

            bid_str = str(bid)
            ask_str = str(ask)
            last = self._last_bbo.get(deltas.instrument_id)
            if last != (bid_str, ask_str):
                self._last_bbo[deltas.instrument_id] = (bid_str, ask_str)
                now_ns = int(self.clock.timestamp_ns())
                self._publish_market_bbo(
                    instrument_id=deltas.instrument_id,
                    bid=bid,
                    ask=ask,
                    ts_ns=now_ns,
                )
                self._recompute_and_publish_fv()
                self._publish_state_if_due()

            self._publish_balances_if_due()

            if self.config.bybit_instrument_id != deltas.instrument_id:
                return

            if not self.config.bot_on:
                self._cancel_managed_quotes("bot_off")
                self._publish_state("bot_off")
                return

            now_ns = int(self.clock.timestamp_ns())
            if now_ns - self._last_requote_ns < self.INTERNAL_REQUOTE_THROTTLE_MS * 1_000_000:
                return
            self._refresh_quotes(now_ns=now_ns)

        def on_order_filled(self, event: OrderFilled) -> None:
            self._publish_json(
                TOPIC_TRADE,
                {
                    "strategy_id": self._external_strategy_id,
                    "event": "order_filled",
                    "instrument_id": str(event.instrument_id),
                    "client_order_id": str(event.client_order_id),
                    "trade_id": str(event.trade_id),
                    "side": str(event.order_side),
                    "qty": str(event.last_qty),
                    "price": str(event.last_px),
                    "ts_event": int(event.ts_event),
                },
            )

        def _refresh_quotes(self, now_ns: int) -> None:
            bybit_mid = self._best_mid(self.config.bybit_instrument_id)
            if bybit_mid is None:
                return

            if self._bybit_instrument is None or self._order_qty is None:
                return

            if self._last_fv is not None:
                fair_value = self._last_fv
            else:
                fair_value = bybit_mid

            spread = self._book_spread(self.config.bybit_instrument_id)
            if spread is None:
                return

            half_spread = spread / Decimal("2")
            anchor_bid = fair_value - half_spread
            anchor_ask = fair_value + half_spread

            bid_targets, ask_targets = build_ladder_targets(
                anchor_bid=anchor_bid,
                anchor_ask=anchor_ask,
                bid_edges=(self.config.bid_edge1, self.config.bid_edge2, self.config.bid_edge3),
                ask_edges=(self.config.ask_edge1, self.config.ask_edge2, self.config.ask_edge3),
                distances=(self.config.distance1, self.config.distance2, self.config.distance3),
                n_orders=(self.config.n_orders1, self.config.n_orders2, self.config.n_orders3),
            )

            desired_orders: list[tuple[OrderSide, Any]] = []
            for bid in bid_targets:
                if bid > 0:
                    desired_orders.append((OrderSide.BUY, self._bybit_instrument.make_price(bid)))
            for ask in ask_targets:
                if ask > 0:
                    desired_orders.append((OrderSide.SELL, self._bybit_instrument.make_price(ask)))

            active_orders = self._managed_orders()
            if not desired_orders:
                self._cancel_managed_quotes("no_targets")
                self._last_requote_ns = now_ns
                return

            if not self._should_replace(active_orders=active_orders, desired_orders=desired_orders, now_ns=now_ns):
                self._last_requote_ns = now_ns
                return

            if active_orders:
                self._cancel_managed_quotes("replace")

            for side, price in desired_orders:
                order = self.order_factory.limit(
                    instrument_id=self.config.bybit_instrument_id,
                    order_side=side,
                    quantity=self._order_qty,
                    price=price,
                    post_only=True,
                )
                self.submit_order(order)

            self._last_requote_ns = now_ns
            self._publish_event(
                "quotes_replaced",
                bid_levels=len([1 for side, _ in desired_orders if side is OrderSide.BUY]),
                ask_levels=len([1 for side, _ in desired_orders if side is OrderSide.SELL]),
            )
            self._publish_state("quotes_replaced")

        def _publish_state_if_due(self) -> None:
            now_ns = int(self.clock.timestamp_ns())
            if now_ns - self._last_state_ns < 250_000_000:
                return
            self._publish_state("running")

        def _publish_balances_if_due(self) -> None:
            now_ns = int(self.clock.timestamp_ns())
            if now_ns - self._last_balances_ns < self.BALANCES_PUBLISH_INTERVAL_MS * 1_000_000:
                return
            self._publish_balances()

        def _should_replace(
            self,
            active_orders: list[Any],
            desired_orders: list[tuple[OrderSide, Any]],
            now_ns: int,
        ) -> bool:
            if len(active_orders) != len(desired_orders):
                return True
            if self._has_stale_orders(active_orders=active_orders, now_ns=now_ns):
                return True

            current = sorted(
                (str(order.side), round(float(order.price), self._price_precision))
                for order in active_orders
            )
            desired = sorted(
                (str(side), round(float(price), self._price_precision))
                for side, price in desired_orders
            )
            return current != desired

        def _has_stale_orders(self, active_orders: list[Any], now_ns: int) -> bool:
            max_age_ns = int(self.config.max_age_ms) * 1_000_000
            for order in active_orders:
                ts_init = int(getattr(order, "ts_init", 0))
                if ts_init > 0 and now_ns - ts_init >= max_age_ns:
                    return True
            return False

        def _managed_orders(self) -> list[Any]:
            return list(
                self.cache.orders_open(
                    instrument_id=self.config.bybit_instrument_id,
                    strategy_id=self.id,
                )
            ) + list(
                self.cache.orders_inflight(
                    instrument_id=self.config.bybit_instrument_id,
                    strategy_id=self.id,
                )
            )

        def _cancel_managed_quotes(self, reason: str) -> None:
            if self._managed_orders():
                self.cancel_all_orders(self.config.bybit_instrument_id)
                self._publish_event("quotes_canceled", reason=reason)

        def _best_mid(self, instrument_id: InstrumentId) -> Decimal | None:
            book = self._books.get(instrument_id)
            if book is None:
                return None
            bid = book.best_bid_price()
            ask = book.best_ask_price()
            if bid is None or ask is None:
                return None
            return (bid.as_decimal() + ask.as_decimal()) / Decimal("2")

        def _book_spread(self, instrument_id: InstrumentId) -> Decimal | None:
            book = self._books.get(instrument_id)
            if book is None:
                return None
            bid = book.best_bid_price()
            ask = book.best_ask_price()
            if bid is None or ask is None:
                return None
            return ask.as_decimal() - bid.as_decimal()

        def _recompute_and_publish_fv(self) -> None:
            bybit_mid = self._best_mid(self.config.bybit_instrument_id)
            binance_mid = self._best_mid(self.config.binance_instrument_id)
            if bybit_mid is None and binance_mid is None:
                return

            if bybit_mid is not None and binance_mid is not None:
                self._last_fv = (bybit_mid + binance_mid) / Decimal("2")
            else:
                self._last_fv = bybit_mid or binance_mid

            now_ns = int(self.clock.timestamp_ns())
            payload = {
                "strategy_id": self._external_strategy_id,
                "fv": str(self._last_fv),
                "bybit_mid": str(bybit_mid) if bybit_mid is not None else None,
                "binance_mid": str(binance_mid) if binance_mid is not None else None,
                "ts_event": now_ns,
                "ts_ms": now_ns // 1_000_000,
            }
            self._publish_json(TOPIC_FV, [payload])
            self._last_fv_snapshot_ts_ns = now_ns

        def _publish_market_bbo(
            self,
            *,
            instrument_id: InstrumentId,
            bid: Any,
            ask: Any,
            ts_ns: int,
        ) -> None:
            instrument_text = str(instrument_id)
            exchange = "binance_spot"
            symbol = instrument_text.split(".", maxsplit=1)[0].replace("-LINEAR", "")
            if ".BYBIT" in instrument_text:
                exchange = "bybit_linear"

            if _CONTRACT_HELPERS_AVAILABLE:
                try:
                    contract = get_instrument_contract(instrument_text)
                    exchange = contract.chainsaw_exchange
                    symbol = contract.chainsaw_symbol
                except Exception:
                    pass

            if "/" in symbol:
                base, quote = symbol.split("/", maxsplit=1)
            else:
                base = symbol
                quote = "USDT"

            if "/" not in symbol:
                symbol = f"{base}/{quote}"

            try:
                fv_coin = make_fv_coin(symbol)
            except Exception:
                fv_coin = f"{str(base).lower()}/{str(quote).lower()}"

            payload = {
                "strategy_id": self._external_strategy_id,
                "instrument_id": instrument_text,
                "exchange": exchange,
                "chainsaw_exchange": exchange,
                "base": base,
                "quote": quote,
                "symbol": symbol,
                "fv_coin": fv_coin,
                "bid": str(bid),
                "ask": str(ask),
                "ts_event": ts_ns,
                "ts_ms": ts_ns // 1_000_000,
            }
            self._publish_json(TOPIC_MARKET_BBO, payload)

        def _publish_state(self, state: str) -> None:
            now_ns = int(self.clock.timestamp_ns())
            self._last_state_ns = now_ns
            self._publish_json(
                TOPIC_STATE,
                {
                    "strategy_id": self._external_strategy_id,
                    "state": state,
                    "bot_on": bool(self.config.bot_on),
                    "managed_orders": len(self._managed_orders()),
                    "ts_event": now_ns,
                    "ts_ms": now_ns // 1_000_000,
                },
            )

        def _publish_event(self, name: str, **payload: Any) -> None:
            now_ns = int(self.clock.timestamp_ns())
            data: dict[str, Any] = {
                "strategy_id": self._external_strategy_id,
                "event": name,
                "ts_event": now_ns,
                "ts_ms": now_ns // 1_000_000,
            }
            data.update(payload)
            self._publish_json(TOPIC_EVENT, data)

        def _publish_alert(self, message: str, level: str = "warning") -> None:
            now_ns = int(self.clock.timestamp_ns())
            self._publish_json(
                TOPIC_ALERT,
                {
                    "strategy_id": self._external_strategy_id,
                    "level": level,
                    "message": message,
                    "ts_event": now_ns,
                    "ts_ms": now_ns // 1_000_000,
                },
            )

        def _publish_balances(self) -> None:
            now_ns = int(self.clock.timestamp_ns())
            self._last_balances_ns = now_ns
            payload: dict[str, Any] = {"strategy_id": self._external_strategy_id, "accounts": []}
            for account in self.cache.accounts():
                if hasattr(account, "to_dict"):
                    payload["accounts"].append(account.to_dict())
                else:
                    payload["accounts"].append({"repr": repr(account)})
            payload["ts_event"] = now_ns
            payload["ts_ms"] = now_ns // 1_000_000
            self._publish_json(TOPIC_BALANCES, payload)

        def _publish_json(self, topic: str, payload: dict[str, Any]) -> None:
            self.msgbus.publish(topic=topic, msg=_to_json_safe(payload))


else:
    class MakerV3SingleLegQuoterConfig:  # pragma: no cover - fallback for pure-math tests
        pass


    class MakerV3SingleLegQuoter:  # pragma: no cover - fallback for pure-math tests
        def __init__(self, *_args: Any, **_kwargs: Any) -> None:
            raise ModuleNotFoundError(
                "NautilusTrader runtime modules are unavailable in this environment",
            ) from _NAUTILUS_IMPORT_ERROR
