from __future__ import annotations

import json
from decimal import Decimal
from typing import Iterable


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
    Build deterministic 3-band bid/ask target ladders from anchor prices.

    For each band ``i`` and level ``j`` (starting from 0):
    - bid price  = ``anchor_bid - bid_edge_i - distance_i * j``
    - ask price  = ``anchor_ask + ask_edge_i + distance_i * j``
    """
    bid_edge_1, bid_edge_2, bid_edge_3 = _validate_three_band_input(bid_edges, "bid_edges")
    ask_edge_1, ask_edge_2, ask_edge_3 = _validate_three_band_input(ask_edges, "ask_edges")
    dist_1, dist_2, dist_3 = _validate_three_band_input(distances, "distances")
    n_1, n_2, n_3 = _validate_three_band_input(n_orders, "n_orders")

    bid_edge_vals = (_to_decimal(bid_edge_1), _to_decimal(bid_edge_2), _to_decimal(bid_edge_3))
    ask_edge_vals = (_to_decimal(ask_edge_1), _to_decimal(ask_edge_2), _to_decimal(ask_edge_3))
    distance_vals = (_to_decimal(dist_1), _to_decimal(dist_2), _to_decimal(dist_3))
    n_orders_vals = (int(n_1), int(n_2), int(n_3))

    if any(distance < 0 for distance in distance_vals):
        raise ValueError("distances must be non-negative")
    if any(edge < 0 for edge in bid_edge_vals + ask_edge_vals):
        raise ValueError("edges must be non-negative")
    if any(levels < 0 for levels in n_orders_vals):
        raise ValueError("n_orders must be non-negative")

    anchor_bid_dec = _to_decimal(anchor_bid)
    anchor_ask_dec = _to_decimal(anchor_ask)

    bid_prices: list[Decimal] = []
    ask_prices: list[Decimal] = []
    for band in range(3):
        for level in range(n_orders_vals[band]):
            step = distance_vals[band] * level
            bid_prices.append(anchor_bid_dec - bid_edge_vals[band] - step)
            ask_prices.append(anchor_ask_dec + ask_edge_vals[band] + step)

    return bid_prices, ask_prices


_NAUTILUS_IMPORT_ERROR: ModuleNotFoundError | None = None
try:
    from nautilus_trader.config import NonNegativeFloat
    from nautilus_trader.config import NonNegativeInt
    from nautilus_trader.config import PositiveInt
    from nautilus_trader.config import StrategyConfig
    from nautilus_trader.model.data import QuoteTick
    from nautilus_trader.model.enums import OrderSide
    from nautilus_trader.model.events import OrderFilled
    from nautilus_trader.model.identifiers import InstrumentId
    from nautilus_trader.model.instruments import Instrument
    from nautilus_trader.model.objects import Quantity
    from nautilus_trader.trading.strategy import Strategy
except ModuleNotFoundError as exc:  # pragma: no cover - local pure-math test fallback
    _NAUTILUS_IMPORT_ERROR = exc


if _NAUTILUS_IMPORT_ERROR is None:
    TOPIC_STATE = "maker_poc.state"
    TOPIC_EVENT = "maker_poc.event"
    TOPIC_TRADE = "maker_poc.trade"
    TOPIC_ALERT = "maker_poc.alert"
    TOPIC_MARKET_BBO = "maker_poc.market_bbo"
    TOPIC_FV = "maker_poc.fv"
    TOPIC_BALANCES = "maker_poc.balances"


    class MakerV3SingleLegQuoterConfig(StrategyConfig, frozen=True):
        """
        Configuration for Maker V3 single-execution-leg 3-band post-only quoting.
        """

        bybit_instrument_id: InstrumentId
        binance_instrument_id: InstrumentId
        order_qty: Decimal
        external_strategy_id: str = "bybit_binance_plumeusdt_makerv3"
        bot_on: bool = True
        max_age_ms: PositiveInt = 2_000
        bid_edge1: NonNegativeFloat = 0.05
        ask_edge1: NonNegativeFloat = 0.05
        distance1: NonNegativeFloat = 0.02
        n_orders1: NonNegativeInt = 1
        bid_edge2: NonNegativeFloat = 0.15
        ask_edge2: NonNegativeFloat = 0.15
        distance2: NonNegativeFloat = 0.04
        n_orders2: NonNegativeInt = 1
        bid_edge3: NonNegativeFloat = 0.35
        ask_edge3: NonNegativeFloat = 0.35
        distance3: NonNegativeFloat = 0.08
        n_orders3: NonNegativeInt = 1


    class MakerV3SingleLegQuoter(Strategy):
        """
        Single-leg post-only quoter:
        - executes only on Bybit linear perp,
        - ingests Bybit + Binance market data,
        - publishes bridge payloads over MessageBus.
        """

        INTERNAL_REQUOTE_THROTTLE_MS = 150

        def __init__(self, config: MakerV3SingleLegQuoterConfig) -> None:
            super().__init__(config)
            self._bybit_instrument: Instrument | None = None
            self._order_qty: Quantity | None = None
            self._price_precision: int = 8
            self._bybit_quote: QuoteTick | None = None
            self._binance_quote: QuoteTick | None = None
            self._last_requote_ns: int = 0
            self._last_fv: Decimal | None = None
            self._external_strategy_id: str = (
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

            self._order_qty = self._bybit_instrument.make_qty(self.config.order_qty)
            self._price_precision = self._bybit_instrument.price_precision

            self.subscribe_quote_ticks(self.config.bybit_instrument_id)
            self.subscribe_quote_ticks(self.config.binance_instrument_id)

            self._publish_event("started")
            self._publish_balances()
            self._publish_state("on_start")

        def on_stop(self) -> None:
            self._cancel_managed_quotes("on_stop")
            self.unsubscribe_quote_ticks(self.config.bybit_instrument_id)
            self.unsubscribe_quote_ticks(self.config.binance_instrument_id)
            self._publish_state("on_stop")

        def on_quote_tick(self, quote: QuoteTick) -> None:
            instrument_id = quote.instrument_id
            if instrument_id == self.config.bybit_instrument_id:
                self._bybit_quote = quote
                source = "bybit"
            elif instrument_id == self.config.binance_instrument_id:
                self._binance_quote = quote
                source = "binance"
            else:
                return

            self._publish_market_bbo(source=source, quote=quote)
            self._recompute_and_publish_fv()

            if not self.config.bot_on:
                self._cancel_managed_quotes("bot_off")
                self._publish_state("bot_off")
                return

            if self._bybit_quote is None:
                return

            now_ns = self.clock.timestamp_ns()
            if now_ns - self._last_requote_ns < self.INTERNAL_REQUOTE_THROTTLE_MS * 1_000_000:
                return

            self._refresh_quotes(now_ns=now_ns)

        def on_order_filled(self, event: OrderFilled) -> None:
            self._publish_json(
                TOPIC_TRADE,
                {
                    "strategy_id": self._external_strategy_id,
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
            if self._bybit_quote is None or self._bybit_instrument is None or self._order_qty is None:
                return

            bybit_bid = self._bybit_quote.bid_price.as_decimal()
            bybit_ask = self._bybit_quote.ask_price.as_decimal()
            if bybit_bid <= 0 or bybit_ask <= bybit_bid:
                self._publish_alert(
                    f"Invalid Bybit BBO bid={bybit_bid} ask={bybit_ask}",
                    level="warning",
                )
                return

            fair_value = self._last_fv if self._last_fv is not None else (bybit_bid + bybit_ask) / Decimal("2")
            half_spread = (bybit_ask - bybit_bid) / Decimal("2")
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

            desired_orders = []
            for bid in bid_targets:
                if bid > 0:
                    desired_orders.append(
                        (OrderSide.BUY, self._bybit_instrument.make_price(bid)),
                    )
            for ask in ask_targets:
                if ask > 0:
                    desired_orders.append(
                        (OrderSide.SELL, self._bybit_instrument.make_price(ask)),
                    )

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
                bid_levels=len([1 for side, _ in desired_orders if side == OrderSide.BUY]),
                ask_levels=len([1 for side, _ in desired_orders if side == OrderSide.SELL]),
                anchor_bid=str(anchor_bid),
                anchor_ask=str(anchor_ask),
            )
            self._publish_state("quotes_replaced")

        def _should_replace(self, active_orders, desired_orders, now_ns: int) -> bool:
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

        def _has_stale_orders(self, active_orders, now_ns: int) -> bool:
            max_age_ns = int(self.config.max_age_ms) * 1_000_000
            for order in active_orders:
                ts_init = int(getattr(order, "ts_init", 0))
                if ts_init > 0 and now_ns - ts_init >= max_age_ns:
                    return True
            return False

        def _managed_orders(self):
            return [
                *self.cache.orders_open(
                    instrument_id=self.config.bybit_instrument_id,
                    strategy_id=self.id,
                ),
                *self.cache.orders_inflight(
                    instrument_id=self.config.bybit_instrument_id,
                    strategy_id=self.id,
                ),
            ]

        def _cancel_managed_quotes(self, reason: str) -> None:
            if self._managed_orders():
                self.cancel_all_orders(self.config.bybit_instrument_id)
                self._publish_event("quotes_canceled", reason=reason)

        def _recompute_and_publish_fv(self) -> None:
            bybit_mid = None
            binance_mid = None
            if self._bybit_quote is not None:
                bybit_mid = (
                    self._bybit_quote.bid_price.as_decimal() + self._bybit_quote.ask_price.as_decimal()
                ) / Decimal("2")
            if self._binance_quote is not None:
                binance_mid = (
                    self._binance_quote.bid_price.as_decimal() + self._binance_quote.ask_price.as_decimal()
                ) / Decimal("2")

            if bybit_mid is None and binance_mid is None:
                return

            if bybit_mid is not None and binance_mid is not None:
                self._last_fv = (bybit_mid + binance_mid) / Decimal("2")
            else:
                self._last_fv = bybit_mid if bybit_mid is not None else binance_mid

            self._publish_json(
                TOPIC_FV,
                {
                    "strategy_id": self._external_strategy_id,
                    "fv": str(self._last_fv),
                    "bybit_mid": str(bybit_mid) if bybit_mid is not None else None,
                    "binance_mid": str(binance_mid) if binance_mid is not None else None,
                    "ts_event": int(self.clock.timestamp_ns()),
                },
            )

        def _publish_market_bbo(self, source: str, quote: QuoteTick) -> None:
            instrument_id = str(quote.instrument_id)
            chainsaw_exchange = "bybit_linear" if ".BYBIT" in instrument_id else "binance_spot"
            symbol_token = instrument_id.split(".", maxsplit=1)[0]
            symbol_root = symbol_token.split("-", maxsplit=1)[0]
            known_quotes = ("USDT", "USDC", "USD", "BTC", "ETH", "EUR", "BNB")
            base = symbol_root
            quote_ccy = "USDT"
            for candidate in known_quotes:
                if symbol_root.endswith(candidate) and len(symbol_root) > len(candidate):
                    base = symbol_root[: -len(candidate)]
                    quote_ccy = candidate
                    break
            slash_symbol = f"{base.lower()}/{quote_ccy.lower()}"

            self._publish_json(
                TOPIC_MARKET_BBO,
                {
                    "strategy_id": self._external_strategy_id,
                    "source": source,
                    "instrument_id": instrument_id,
                    "exchange": chainsaw_exchange,
                    "base": base,
                    "quote": quote_ccy,
                    "symbol": slash_symbol,
                    "bid": str(quote.bid_price),
                    "ask": str(quote.ask_price),
                    "ts_event": int(quote.ts_event),
                    "ts_init": int(quote.ts_init),
                },
            )

        def _publish_state(self, state: str) -> None:
            self._publish_json(
                TOPIC_STATE,
                {
                    "strategy_id": self._external_strategy_id,
                    "state": state,
                    "bot_on": bool(self.config.bot_on),
                    "managed_orders": len(self._managed_orders()),
                    "ts_event": int(self.clock.timestamp_ns()),
                },
            )

        def _publish_event(self, name: str, **payload) -> None:
            data = {
                "strategy_id": self._external_strategy_id,
                "event": name,
                "ts_event": int(self.clock.timestamp_ns()),
            }
            data.update(payload)
            self._publish_json(TOPIC_EVENT, data)

        def _publish_alert(self, message: str, level: str = "warning") -> None:
            self._publish_json(
                TOPIC_ALERT,
                {
                    "strategy_id": self._external_strategy_id,
                    "level": level,
                    "message": message,
                    "ts_event": int(self.clock.timestamp_ns()) if self.clock is not None else 0,
                },
            )

        def _publish_balances(self) -> None:
            payload = {"strategy_id": self._external_strategy_id, "accounts": []}
            for account in self.cache.accounts():
                if hasattr(account, "to_dict"):
                    payload["accounts"].append(account.to_dict())
                else:
                    payload["accounts"].append({"repr": repr(account)})
            payload["ts_event"] = int(self.clock.timestamp_ns())
            self._publish_json(TOPIC_BALANCES, payload)

        def _publish_json(self, topic: str, payload: dict) -> None:
            self.msgbus.publish(topic=topic, msg=json.dumps(payload, separators=(",", ":"), sort_keys=True))


else:
    class MakerV3SingleLegQuoterConfig:  # pragma: no cover - fallback for pure-math tests
        pass


    class MakerV3SingleLegQuoter:  # pragma: no cover - fallback for pure-math tests
        def __init__(self, *_args, **_kwargs) -> None:
            raise ModuleNotFoundError(
                "NautilusTrader runtime modules are unavailable in this environment",
            ) from _NAUTILUS_IMPORT_ERROR
