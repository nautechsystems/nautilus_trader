#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import datetime as dt
import json
import os
from typing import Any
from uuid import uuid4

from nautilus_trader.core import nautilus_pyo3 as pyo3


def env_bool(name: str, default: bool = False) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.lower() in {"1", "true", "yes", "y"}


def env_int(name: str, default: int) -> int:
    return int(os.getenv(name, str(default)))


def env_price(name: str, default: str) -> pyo3.Price:
    return pyo3.Price.from_str(os.getenv(name, default))


def env_quantity(name: str, default: str = "1") -> pyo3.Quantity:
    return pyo3.Quantity.from_str(os.getenv(name, default))


def env_order_side(name: str, default: pyo3.OrderSide) -> pyo3.OrderSide:
    value = os.getenv(name)
    if value is None:
        return default

    value = value.upper()
    if value == "BUY":
        return pyo3.OrderSide.BUY
    if value == "SELL":
        return pyo3.OrderSide.SELL
    raise ValueError(f"{name} must be BUY or SELL")


def env_ib_oca_type(name: str, default: Any) -> int:
    value = os.getenv(name)
    if value is not None:
        return int(value)
    return default.as_i32()


def env_ib_trigger_method(name: str, default: Any) -> int:
    value = os.getenv(name)
    if value is not None:
        return int(value)
    return default.as_i32()


def env_instrument_id(name: str, default: str) -> pyo3.InstrumentId:
    return pyo3.InstrumentId.from_str(os.getenv(name, default))


def ib_order_tags(**values: object) -> str:
    return "IBOrderTags:" + json.dumps(values, separators=(",", ":"), sort_keys=True)


def ib_client_id() -> pyo3.ClientId:
    return pyo3.ClientId.from_str("IB")


def databento_client_id() -> pyo3.ClientId:
    return pyo3.ClientId.from_str("DATABENTO")


def bar_type_from_env(name: str, instrument_id: pyo3.InstrumentId) -> pyo3.BarType:
    return pyo3.BarType.from_str(
        os.getenv(name, f"{instrument_id}-1-MINUTE-LAST-EXTERNAL"),
    )


def contract_id_from_instrument(instrument: Any | None) -> int:
    info = getattr(instrument, "info", None)
    if not isinstance(info, dict):
        return 0

    contract = info.get("contract")
    if not isinstance(contract, dict):
        return 0

    return int(contract.get("conId") or 0)


class IbV2SubscriptionStrategy(pyo3.Strategy):  # type: ignore[name-defined]
    def __init__(self) -> None:
        super().__init__(
            pyo3.StrategyConfig(  # type: ignore[attr-defined]
                strategy_id=pyo3.StrategyId.from_str("IB-V2-SUBSCRIPTION-STRATEGY"),
            ),
        )
        self.instrument_id = env_instrument_id("IB_V2_SUBSCRIPTION_INSTRUMENT_ID", "^SPX.CBOE")
        self.bar_type = bar_type_from_env("IB_V2_SUBSCRIPTION_BAR_TYPE", self.instrument_id)
        self._subscribed = False
        self._quote_count = 0
        self._trade_count = 0
        self._bar_count = 0
        self._index_price_count = 0
        self._max_prints = env_int("IB_V2_SUBSCRIPTION_MAX_PRINTS", 5)

    def on_start(self) -> None:
        print(f"{self.strategy_id}: requesting {self.instrument_id}", flush=True)
        self.request_instrument(self.instrument_id, client_id=ib_client_id())

    def on_instrument(self, instrument: Any) -> None:
        if instrument.id != self.instrument_id:
            return

        self._subscribe_once()

    def _subscribe_once(self) -> None:
        if self._subscribed:
            return

        self._subscribed = True
        if env_bool("IB_V2_SUBSCRIBE_QUOTES"):
            print(f"{self.strategy_id}: subscribing quotes for {self.instrument_id}", flush=True)
            self.subscribe_quotes(self.instrument_id, client_id=ib_client_id())

        if env_bool("IB_V2_SUBSCRIBE_TRADES"):
            print(f"{self.strategy_id}: subscribing trades for {self.instrument_id}", flush=True)
            self.subscribe_trades(self.instrument_id, client_id=ib_client_id())

        if env_bool("IB_V2_SUBSCRIBE_BARS"):
            print(f"{self.strategy_id}: subscribing bars for {self.bar_type}", flush=True)
            self.subscribe_bars(self.bar_type, client_id=ib_client_id())

        if env_bool("IB_V2_SUBSCRIBE_INDEX_PRICES"):
            print(
                f"{self.strategy_id}: subscribing index prices for {self.instrument_id}",
                flush=True,
            )
            self.subscribe_index_prices(self.instrument_id, client_id=ib_client_id())

    def on_quote(self, tick: Any) -> None:
        self._quote_count += 1
        if self._quote_count <= self._max_prints:
            print(f"{self.strategy_id}: quote #{self._quote_count}: {tick}", flush=True)

    def on_trade(self, tick: Any) -> None:
        self._trade_count += 1
        if self._trade_count <= self._max_prints:
            print(f"{self.strategy_id}: trade #{self._trade_count}: {tick}", flush=True)

    def on_bar(self, bar: Any) -> None:
        self._bar_count += 1
        if self._bar_count <= self._max_prints:
            print(f"{self.strategy_id}: bar #{self._bar_count}: {bar}", flush=True)

    def on_index_price(self, index_price: Any) -> None:
        self._index_price_count += 1
        if self._index_price_count <= self._max_prints:
            print(
                f"{self.strategy_id}: index price #{self._index_price_count}: {index_price}",
                flush=True,
            )


class DatabentoSubscriptionStrategy(pyo3.Strategy):  # type: ignore[name-defined]
    def __init__(self) -> None:
        super().__init__(
            pyo3.StrategyConfig(  # type: ignore[attr-defined]
                strategy_id=pyo3.StrategyId.from_str("IB-V2-DATABENTO-SUBSCRIPTION"),
            ),
        )
        self.instrument_id = env_instrument_id("IB_V2_DATABENTO_DATA_INSTRUMENT_ID", "SPY.XNAS")
        self.bar_type = bar_type_from_env("IB_V2_DATABENTO_BAR_TYPE", self.instrument_id)
        self._quote_count = 0
        self._bar_count = 0
        self._max_prints = env_int("IB_V2_SUBSCRIPTION_MAX_PRINTS", 5)

    def on_start(self) -> None:
        if env_bool("IB_V2_DATABENTO_SUBSCRIBE_QUOTES", True):
            print(
                f"{self.strategy_id}: subscribing Databento quotes for {self.instrument_id}",
                flush=True,
            )
            self.subscribe_quotes(self.instrument_id, client_id=databento_client_id())

        if env_bool("IB_V2_DATABENTO_SUBSCRIBE_BARS", True):
            print(f"{self.strategy_id}: subscribing Databento bars for {self.bar_type}", flush=True)
            self.subscribe_bars(self.bar_type, client_id=databento_client_id())

    def on_quote(self, tick: Any) -> None:
        self._quote_count += 1
        if self._quote_count <= self._max_prints:
            print(f"{self.strategy_id}: Databento quote #{self._quote_count}: {tick}", flush=True)

    def on_bar(self, bar: Any) -> None:
        self._bar_count += 1
        if self._bar_count <= self._max_prints:
            print(f"{self.strategy_id}: Databento bar #{self._bar_count}: {bar}", flush=True)


class OptionGreeksStrategy(pyo3.Strategy):  # type: ignore[name-defined]
    def __init__(self) -> None:
        super().__init__(
            pyo3.StrategyConfig(  # type: ignore[attr-defined]
                strategy_id=pyo3.StrategyId.from_str("IB-V2-OPTION-GREEKS-STRATEGY"),
            ),
        )
        self.instrument_id = env_instrument_id(
            "IB_V2_OPTION_INSTRUMENT_ID",
            "ESM6 P6800.IB",
        )
        self._subscribed = False
        self._count = 0
        self._max_prints = env_int("IB_V2_OPTION_GREEKS_MAX_PRINTS", 5)

    def on_start(self) -> None:
        print(f"{self.strategy_id}: requesting option {self.instrument_id}", flush=True)
        self.request_instrument(self.instrument_id, client_id=ib_client_id())

    def on_instrument(self, instrument: Any) -> None:
        if instrument.id != self.instrument_id or self._subscribed:
            return

        self._subscribed = True
        print(f"{self.strategy_id}: subscribing option greeks for {self.instrument_id}", flush=True)
        self.subscribe_option_greeks(self.instrument_id, client_id=ib_client_id())

    def on_option_greeks(self, greeks: Any) -> None:
        self._count += 1
        if self._count <= self._max_prints:
            print(f"{self.strategy_id}: option greeks #{self._count}: {greeks}", flush=True)

    def on_stop(self) -> None:
        if self._subscribed:
            self.unsubscribe_option_greeks(self.instrument_id, client_id=ib_client_id())


class IbV2OrderStrategy(pyo3.Strategy):  # type: ignore[name-defined]
    strategy_id_value = "IB-V2-ORDER-001"
    instrument_id_value = "ESM6.IB"

    def __init__(self) -> None:
        super().__init__(
            pyo3.StrategyConfig(  # type: ignore[attr-defined]
                strategy_id=pyo3.StrategyId.from_str(self.strategy_id_value),
                manage_contingent_orders=True,
            ),
        )
        self.instrument_id = env_instrument_id(
            "IB_V2_ORDER_INSTRUMENT_ID",
            self.instrument_id_value,
        )
        self.instrument: Any | None = None
        self._orders_submitted = False

    def on_start(self) -> None:
        if not env_bool("IB_V2_ENABLE_ORDER_SUBMISSION"):
            print(
                f"{self.strategy_id}: registered; set IB_V2_ENABLE_ORDER_SUBMISSION=1 to submit.",
                flush=True,
            )
            return

        cached_instrument = self.cache.instrument(self.instrument_id)
        if cached_instrument is not None:
            self.instrument = cached_instrument
            self._submit_example_orders_once()
            return

        print(f"{self.strategy_id}: requesting {self.instrument_id}", flush=True)
        self.request_instrument(self.instrument_id, client_id=ib_client_id())

    def on_instrument(self, instrument: Any) -> None:
        if instrument.id != self.instrument_id:
            return

        self.instrument = instrument
        self._submit_example_orders_once()

    def _submit_example_orders_once(self) -> None:
        if self._orders_submitted:
            return

        self._orders_submitted = True
        print(f"{self.strategy_id}: submitting orders for {self.instrument_id}", flush=True)
        self.submit_example_orders()

    def submit_example_orders(self) -> None:
        raise NotImplementedError

    def client_order_id(self, suffix: str) -> pyo3.ClientOrderId:
        return pyo3.ClientOrderId.from_str(f"{self.strategy_id}-{suffix}")

    def init_id(self) -> pyo3.UUID4:
        return pyo3.UUID4.from_str(str(uuid4()))

    def timestamp_ns(self) -> int:
        return self.clock.timestamp_ns()

    def market_order(
        self,
        client_order_id: pyo3.ClientOrderId,
        order_side: pyo3.OrderSide,
        quantity: pyo3.Quantity,
        tags: list[str] | None = None,
        contingency_type: pyo3.ContingencyType | None = None,
        order_list_id: pyo3.OrderListId | None = None,
        linked_order_ids: list[pyo3.ClientOrderId] | None = None,
        parent_order_id: pyo3.ClientOrderId | None = None,
    ) -> pyo3.MarketOrder:
        trader_id = self.trader_id
        if trader_id is None:
            raise RuntimeError("Strategy is not registered")

        return pyo3.MarketOrder(
            trader_id,
            self.strategy_id,
            self.instrument_id,
            client_order_id,
            order_side,
            quantity,
            self.init_id(),
            self.timestamp_ns(),
            pyo3.TimeInForce.DAY,
            False,
            False,
            contingency_type=contingency_type,
            order_list_id=order_list_id,
            linked_order_ids=linked_order_ids,
            parent_order_id=parent_order_id,
            tags=tags,
        )

    def limit_order(
        self,
        client_order_id: pyo3.ClientOrderId,
        order_side: pyo3.OrderSide,
        quantity: pyo3.Quantity,
        price: pyo3.Price,
        time_in_force: pyo3.TimeInForce = pyo3.TimeInForce.DAY,
        tags: list[str] | None = None,
        contingency_type: pyo3.ContingencyType | None = None,
        order_list_id: pyo3.OrderListId | None = None,
        linked_order_ids: list[pyo3.ClientOrderId] | None = None,
        parent_order_id: pyo3.ClientOrderId | None = None,
    ) -> pyo3.LimitOrder:
        trader_id = self.trader_id
        if trader_id is None:
            raise RuntimeError("Strategy is not registered")

        return pyo3.LimitOrder(
            trader_id,
            self.strategy_id,
            self.instrument_id,
            client_order_id,
            order_side,
            quantity,
            price,
            time_in_force,
            False,
            False,
            False,
            self.init_id(),
            self.timestamp_ns(),
            contingency_type=contingency_type,
            order_list_id=order_list_id,
            linked_order_ids=linked_order_ids,
            parent_order_id=parent_order_id,
            tags=tags,
        )

    def stop_market_order(
        self,
        client_order_id: pyo3.ClientOrderId,
        order_side: pyo3.OrderSide,
        quantity: pyo3.Quantity,
        trigger_price: pyo3.Price,
        tags: list[str] | None = None,
        contingency_type: pyo3.ContingencyType | None = None,
        order_list_id: pyo3.OrderListId | None = None,
        linked_order_ids: list[pyo3.ClientOrderId] | None = None,
        parent_order_id: pyo3.ClientOrderId | None = None,
    ) -> pyo3.StopMarketOrder:
        trader_id = self.trader_id
        if trader_id is None:
            raise RuntimeError("Strategy is not registered")

        return pyo3.StopMarketOrder(
            trader_id,
            self.strategy_id,
            self.instrument_id,
            client_order_id,
            order_side,
            quantity,
            trigger_price,
            pyo3.TriggerType.DEFAULT,
            pyo3.TimeInForce.DAY,
            False,
            False,
            self.init_id(),
            self.timestamp_ns(),
            contingency_type=contingency_type,
            order_list_id=order_list_id,
            linked_order_ids=linked_order_ids,
            parent_order_id=parent_order_id,
            tags=tags,
        )

    def submit_ib_order(self, order: object) -> None:
        self.submit_order(order, client_id=ib_client_id())

    def on_order_accepted(self, event: Any) -> None:
        if not env_bool("IB_V2_CANCEL_ON_ACCEPT", True):
            return

        order = self.cache.order(event.client_order_id)
        if order is None:
            print(f"{self.strategy_id}: unable to auto-cancel {event.client_order_id}", flush=True)
            return

        print(
            f"{self.strategy_id}: auto-canceling accepted order {event.client_order_id}",
            flush=True,
        )
        self.cancel_order(order.client_order_id, client_id=ib_client_id())


class BracketOrderStrategy(IbV2OrderStrategy):
    strategy_id_value = "IB-V2-BRACKET-STRATEGY"

    def submit_example_orders(self) -> None:
        quantity = env_quantity("IB_V2_BRACKET_QUANTITY")
        entry_id = self.client_order_id("ENTRY")
        target_id = self.client_order_id("TARGET")
        stop_id = self.client_order_id("STOP")
        order_list_id = pyo3.OrderListId.from_str(f"{self.strategy_id}-BRACKET")
        linked_ids = [entry_id, target_id, stop_id]

        entry = self.market_order(
            entry_id,
            pyo3.OrderSide.BUY,
            quantity,
            order_list_id=order_list_id,
            linked_order_ids=linked_ids,
        )
        target = self.limit_order(
            target_id,
            pyo3.OrderSide.SELL,
            quantity,
            env_price("IB_V2_BRACKET_TARGET_PRICE", "6025.00"),
            contingency_type=pyo3.ContingencyType.OTO,
            order_list_id=order_list_id,
            linked_order_ids=linked_ids,
            parent_order_id=entry_id,
        )
        stop = self.stop_market_order(
            stop_id,
            pyo3.OrderSide.SELL,
            quantity,
            env_price("IB_V2_BRACKET_STOP_PRICE", "5975.00"),
            contingency_type=pyo3.ContingencyType.OTO,
            order_list_id=order_list_id,
            linked_order_ids=linked_ids,
            parent_order_id=entry_id,
        )

        self.submit_ib_order(entry)
        self.submit_ib_order(target)
        self.submit_ib_order(stop)


class MarketOrderStrategy(IbV2OrderStrategy):
    strategy_id_value = "IB-V2-MARKET-STRATEGY"

    def submit_example_orders(self) -> None:
        order = self.market_order(
            self.client_order_id("MARKET"),
            env_order_side("IB_V2_MARKET_SIDE", pyo3.OrderSide.BUY),
            env_quantity("IB_V2_MARKET_QUANTITY"),
        )
        self.submit_ib_order(order)


class OcaGroupStrategy(IbV2OrderStrategy):
    strategy_id_value = "IB-V2-OCA-STRATEGY"

    def submit_example_orders(self) -> None:
        ib = pyo3.interactive_brokers
        quantity = env_quantity("IB_V2_OCA_QUANTITY")
        oca_group = os.getenv("IB_V2_OCA_GROUP", f"TEST_OCA_V2_{uuid4().hex[:12]}")
        tag = ib_order_tags(
            ocaGroup=oca_group,
            ocaType=env_ib_oca_type("IB_V2_OCA_TYPE", ib.IbOcaType.CANCEL_WITH_BLOCK),
        )
        buy = self.limit_order(
            self.client_order_id("BUY"),
            pyo3.OrderSide.BUY,
            quantity,
            env_price("IB_V2_OCA_BUY_PRICE", "5975.00"),
            tags=[tag],
        )
        sell = self.limit_order(
            self.client_order_id("SELL"),
            pyo3.OrderSide.SELL,
            quantity,
            env_price("IB_V2_OCA_SELL_PRICE", "8500.00"),
            tags=[tag],
        )

        self.submit_ib_order(buy)
        self.submit_ib_order(sell)


class SimpleConditionsStrategy(IbV2OrderStrategy):
    strategy_id_value = "IB-V2-CONDITIONS-STRATEGY"

    def submit_example_orders(self) -> None:
        ib = pyo3.interactive_brokers
        time_str = (dt.datetime.now(dt.UTC) + dt.timedelta(minutes=5)).strftime("%Y%m%d-%H:%M:%S")
        time_condition = {
            "type": ib.IbConditionKind.TIME.as_str(),
            "time": time_str,
            "isMore": True,
            "conjunction": ib.IbConditionConjunction.AND.as_str(),
        }
        time_order = self.limit_order(
            self.client_order_id("TIME-CONDITION"),
            pyo3.OrderSide.SELL,
            env_quantity("IB_V2_CONDITION_QUANTITY"),
            env_price("IB_V2_CONDITION_TIME_LIMIT_PRICE", "6100.00"),
            pyo3.TimeInForce.GTC,
            tags=[
                ib_order_tags(
                    conditions=[time_condition],
                    conditionsCancelOrder=env_bool("IB_V2_CONDITIONS_CANCEL_ORDER", False),
                ),
            ],
        )
        self.submit_ib_order(time_order)

        if not env_bool("IB_V2_ENABLE_PRICE_CONDITION", True):
            return

        con_id = env_int("IB_V2_CONDITION_CONTRACT_ID", 0) or contract_id_from_instrument(
            self.instrument,
        )

        if con_id <= 0:
            print(
                f"{self.strategy_id}: skipping price condition because IB contract conId is missing",
                flush=True,
            )
            return

        price_condition = {
            "type": ib.IbConditionKind.PRICE.as_str(),
            "conId": con_id,
            "exchange": os.getenv("IB_V2_CONDITION_EXCHANGE", "CME"),
            "isMore": True,
            "price": float(os.getenv("IB_V2_CONDITION_TRIGGER_PRICE", "6000.0")),
            "triggerMethod": env_ib_trigger_method(
                "IB_V2_CONDITION_TRIGGER_METHOD",
                ib.IbTriggerMethod.DEFAULT,
            ),
            "conjunction": ib.IbConditionConjunction.AND.as_str(),
        }
        price_order = self.limit_order(
            self.client_order_id("PRICE-CONDITION"),
            pyo3.OrderSide.BUY,
            env_quantity("IB_V2_CONDITION_QUANTITY"),
            env_price("IB_V2_CONDITION_PRICE_LIMIT_PRICE", "5950.00"),
            pyo3.TimeInForce.GTC,
            tags=[
                ib_order_tags(
                    conditions=[price_condition],
                    conditionsCancelOrder=env_bool("IB_V2_CONDITIONS_CANCEL_ORDER", False),
                ),
            ],
        )
        self.submit_ib_order(price_order)

        if env_bool("IB_V2_SUBMIT_COMBINED_CONDITION_ORDER"):
            combined_order = self.market_order(
                self.client_order_id("COMBINED-CONDITIONAL"),
                pyo3.OrderSide.BUY,
                env_quantity("IB_V2_CONDITION_QUANTITY"),
                tags=[
                    ib_order_tags(
                        conditions=[time_condition, price_condition],
                        conditionsCancelOrder=env_bool("IB_V2_CONDITIONS_CANCEL_ORDER", False),
                    ),
                ],
            )
            self.submit_ib_order(combined_order)


class SpreadOrderStrategy(IbV2OrderStrategy):
    strategy_id_value = "IB-V2-SPREAD-STRATEGY"
    instrument_id_value = "ESM6P6800.IB"

    def __init__(self) -> None:
        super().__init__()
        self._flatten_submitted = False

    def submit_example_orders(self) -> None:
        ib = pyo3.interactive_brokers
        order = self.market_order(
            self.client_order_id("SPREAD"),
            pyo3.OrderSide.BUY,
            env_quantity("IB_V2_SPREAD_QUANTITY"),
            tags=[
                ib_order_tags(
                    spreadLegs=[
                        {
                            "localSymbol": "ESM6 P6800",
                            "action": ib.IbLegAction.BUY.as_str(),
                            "ratio": 1,
                        },
                        {
                            "localSymbol": "ESM6 P6775",
                            "action": ib.IbLegAction.SELL.as_str(),
                            "ratio": 1,
                        },
                    ],
                ),
            ],
        )
        self.submit_ib_order(order)

    def on_order_submitted(self, event: Any) -> None:
        print(f"{self.strategy_id}: order submitted: {event}", flush=True)

    def on_order_rejected(self, event: Any) -> None:
        print(f"{self.strategy_id}: order rejected: {event}", flush=True)

    def on_order_filled(self, event: Any) -> None:
        print(f"{self.strategy_id}: order filled: {event}", flush=True)
        if not env_bool("IB_V2_SPREAD_FLATTEN_ON_FILL") or self._flatten_submitted:
            return

        if event.instrument_id != self.instrument_id:
            return

        self._flatten_submitted = True
        flatten = self.market_order(
            self.client_order_id("SPREAD-FLATTEN"),
            pyo3.OrderSide.SELL,
            event.last_qty,
        )
        self.submit_ib_order(flatten)


class DatabentoInstrumentIdStrategy(IbV2OrderStrategy):
    strategy_id_value = "IB-V2-DB-ID-STRATEGY"
    instrument_id_value = "YMM6.XCBT"

    def __init__(self) -> None:
        super().__init__()
        self.instrument_id = env_instrument_id(
            "IB_V2_DATABENTO_INSTRUMENT_ID",
            self.instrument_id_value,
        )
        self.bar_type = bar_type_from_env("IB_V2_DATABENTO_INSTRUMENT_BAR_TYPE", self.instrument_id)
        self._seen_instrument_ids: set[str] = set()
        self._startup_requested = False
        self._live_trades_subscribed = False
        self._trade_count = 0
        self._bar_count = 0
        self._max_prints = env_int("IB_V2_SUBSCRIPTION_MAX_PRINTS", 5)

    def on_start(self) -> None:
        print(f"{self.strategy_id}: requesting {self.instrument_id}", flush=True)
        self.request_instrument(self.instrument_id, client_id=ib_client_id())

        contracts = os.getenv("IB_V2_DATABENTO_REQUEST_CONTRACTS")
        if contracts:
            self.request_instruments(
                client_id=ib_client_id(),
                params={"ib_contracts": contracts},
            )

    def on_instrument(self, instrument: Any) -> None:
        instrument_id = str(instrument.id)
        if instrument_id not in self._seen_instrument_ids:
            self._seen_instrument_ids.add(instrument_id)
            print(f"{self.strategy_id}: received instrument: {instrument.id}", flush=True)

        if instrument.id != self.instrument_id or self._startup_requested:
            return

        self.instrument = instrument
        self._startup_requested = True
        start_ns = self.clock.timestamp_ns() - (30 * 60 * 1_000_000_000)
        print(f"{self.strategy_id}: requesting historical bars for {self.bar_type}", flush=True)
        self.request_bars(self.bar_type, start=start_ns, client_id=ib_client_id())

        if env_bool("IB_V2_ENABLE_LIVE_TRADES"):
            self._live_trades_subscribed = True
            print(f"{self.strategy_id}: subscribing trades for {self.instrument_id}", flush=True)
            self.subscribe_trades(self.instrument_id, client_id=ib_client_id())

        if env_bool("IB_V2_ENABLE_LIVE_BARS"):
            print(f"{self.strategy_id}: subscribing live bars for {self.bar_type}", flush=True)
            self.subscribe_bars(
                self.bar_type,
                client_id=ib_client_id(),
                params={"start_ns": str(start_ns)},
            )

        if env_bool("IB_V2_ENABLE_ORDER_SUBMISSION"):
            self.submit_example_orders()

    def submit_example_orders(self) -> None:
        if self._orders_submitted:
            return

        self._orders_submitted = True
        quantity = env_quantity("IB_V2_DATABENTO_BRACKET_QUANTITY")
        entry_id = self.client_order_id("ENTRY")
        target_id = self.client_order_id("TARGET")
        stop_id = self.client_order_id("STOP")
        order_list_id = pyo3.OrderListId.from_str(f"{self.strategy_id}-BRACKET")
        linked_ids = [entry_id, target_id, stop_id]
        entry = self.market_order(
            entry_id,
            pyo3.OrderSide.BUY,
            quantity,
            order_list_id=order_list_id,
            linked_order_ids=linked_ids,
        )
        target = self.limit_order(
            target_id,
            pyo3.OrderSide.SELL,
            quantity,
            env_price("IB_V2_DATABENTO_BRACKET_TARGET_PRICE", "46755.00"),
            pyo3.TimeInForce.GTC,
            contingency_type=pyo3.ContingencyType.OTO,
            order_list_id=order_list_id,
            linked_order_ids=linked_ids,
            parent_order_id=entry_id,
        )
        stop = self.stop_market_order(
            stop_id,
            pyo3.OrderSide.SELL,
            quantity,
            env_price("IB_V2_DATABENTO_BRACKET_STOP_PRICE", "46735.00"),
            contingency_type=pyo3.ContingencyType.OTO,
            order_list_id=order_list_id,
            linked_order_ids=linked_ids,
            parent_order_id=entry_id,
        )
        self.submit_ib_order(entry)
        self.submit_ib_order(target)
        self.submit_ib_order(stop)

    def on_trade(self, trade: Any) -> None:
        self._trade_count += 1
        if self._trade_count <= self._max_prints:
            print(f"{self.strategy_id}: trade #{self._trade_count}: {trade}", flush=True)

    def on_bar(self, bar: Any) -> None:
        self._bar_count += 1
        if self._bar_count <= self._max_prints:
            print(f"{self.strategy_id}: bar #{self._bar_count}: {bar}", flush=True)

    def on_historical_bars(self, bars: list[Any]) -> None:
        print(f"{self.strategy_id}: received {len(bars)} historical bar(s)", flush=True)

    def on_position_opened(self, event: Any) -> None:
        print(f"{self.strategy_id}: position opened: {event}", flush=True)

    def on_stop(self) -> None:
        if self._live_trades_subscribed:
            self.unsubscribe_trades(self.instrument_id, client_id=ib_client_id())

        if self._startup_requested and env_bool("IB_V2_ENABLE_LIVE_BARS"):
            self.unsubscribe_bars(self.bar_type, client_id=ib_client_id())
