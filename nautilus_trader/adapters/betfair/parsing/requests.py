# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import datetime
from functools import lru_cache
from typing import Optional, Union

import pandas as pd

from nautilus_trader.adapters.betfair.common import B2N_ORDER_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import B2N_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.common import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.common import BETFAIR_TICK_SCHEME
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.common import MAX_BET_PROB
from nautilus_trader.adapters.betfair.common import MIN_BET_PROB
from nautilus_trader.adapters.betfair.common import N2B_SIDE
from nautilus_trader.adapters.betfair.common import N2B_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.common import probability_to_price
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.c_enums.order_type import OrderTypeParser
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.market import MarketOrder


def make_custom_order_ref(client_order_id: ClientOrderId, strategy_id: StrategyId) -> str:
    return client_order_id.value.rsplit("-" + strategy_id.get_tag(), maxsplit=1)[0]


def _probability_to_price(probability: Price, side: OrderSide):
    if side == OrderSide.BUY:
        tick_prob = BETFAIR_TICK_SCHEME.next_bid_price(value=probability)
    elif side == OrderSide.SELL:
        tick_prob = BETFAIR_TICK_SCHEME.next_ask_price(value=probability)
    else:
        raise RuntimeError(f"invalid `OrderSide`, was {side}")
    return probability_to_price(probability=tick_prob)


def _order_quantity_to_stake(quantity: Quantity) -> str:
    """
    Convert quantities from nautilus into liabilities in Betfair.
    """
    return str(quantity.as_double())


def _make_limit_order(order: Union[LimitOrder, MarketOrder]):
    price = str(float(_probability_to_price(probability=order.price, side=order.side)))
    size = _order_quantity_to_stake(quantity=order.quantity)

    if order.time_in_force == TimeInForce.AT_THE_CLOSE:
        return {
            "orderType": "LIMIT_ON_CLOSE",
            "limitOnCloseOrder": {"price": price, "liability": size},
        }
    else:
        parsed = {
            "orderType": "LIMIT",
            "limitOrder": {"price": price, "size": size, "persistenceType": "PERSIST"},
        }
        if order.time_in_force in N2B_TIME_IN_FORCE:
            parsed["limitOrder"]["timeInForce"] = N2B_TIME_IN_FORCE[order.time_in_force]  # type: ignore
            parsed["limitOrder"]["persistenceType"] = "LAPSE"  # type: ignore
        return parsed


def _make_market_order(order: Union[LimitOrder, MarketOrder]):
    if order.time_in_force == TimeInForce.AT_THE_CLOSE:
        return {
            "orderType": "MARKET_ON_CLOSE",
            "marketOnCloseOrder": {
                "liability": str(order.quantity.as_double()),
            },
        }
    else:
        # Betfair doesn't really support market orders, return a limit order with min/max price
        limit_order = LimitOrder(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            order_side=order.side,
            quantity=order.quantity,
            price=MAX_BET_PROB if order.side == OrderSide.BUY else MIN_BET_PROB,
            time_in_force=TimeInForce.FOK,
            init_id=order.init_id,
            ts_init=order.ts_init,
        )
        limit_order = _make_limit_order(order=limit_order)
        # We transform the size of a limit order inside `_make_limit_order` but for a market order we want to just use
        # the size as is.
        limit_order["limitOrder"]["size"] = str(order.quantity.as_double())
        return limit_order


def make_order(order: Union[LimitOrder, MarketOrder]):
    if isinstance(order, LimitOrder):
        return _make_limit_order(order=order)
    elif isinstance(order, MarketOrder):
        return _make_market_order(order=order)
    else:
        raise TypeError(f"Unknown order type: {type(order)}")


def order_submit_to_betfair(command: SubmitOrder, instrument: BettingInstrument) -> dict:
    """
    Convert a SubmitOrder command into the data required by BetfairClient.
    """
    order = make_order(command.order)

    place_order = {
        "market_id": instrument.market_id,
        # Used to de-dupe orders on betfair server side
        "customer_ref": command.id.value.replace("-", ""),
        "customer_strategy_ref": command.strategy_id.value[:15],
        "instructions": [
            {
                **order,
                "selectionId": instrument.selection_id,
                "side": N2B_SIDE[command.order.side],
                "handicap": instrument.selection_handicap,
                # Remove the strategy name from customer_order_ref; it has a limited size and we don't control what
                # length the strategy might be or what characters users might append
                "customerOrderRef": make_custom_order_ref(
                    client_order_id=command.order.client_order_id,
                    strategy_id=command.strategy_id,
                ),
            },
        ],
    }
    return place_order


def order_update_to_betfair(
    command: ModifyOrder,
    venue_order_id: VenueOrderId,
    side: OrderSide,
    instrument: BettingInstrument,
):
    """
    Convert an ModifyOrder command into the data required by BetfairClient.
    """
    return {
        "market_id": instrument.market_id,
        "customer_ref": command.id.value.replace("-", ""),
        "instructions": [
            {
                "betId": venue_order_id.value,
                "newPrice": float(_probability_to_price(probability=command.price, side=side)),
            },
        ],
    }


def order_cancel_to_betfair(command: CancelOrder, instrument: BettingInstrument):
    """
    Convert a CancelOrder command into the data required by BetfairClient.
    """
    return {
        "market_id": instrument.market_id,
        "customer_ref": command.id.value.replace("-", ""),
        "instructions": [{"betId": command.venue_order_id.value}],
    }


def order_cancel_all_to_betfair(instrument: BettingInstrument):
    """
    Convert a CancelAllOrders command into the data required by BetfairClient.
    """
    return {
        "market_id": instrument.market_id,
    }


def betfair_account_to_account_state(
    account_detail,
    account_funds,
    event_id,
    ts_event,
    ts_init,
    account_id="001",
) -> AccountState:
    currency = Currency.from_str(account_detail["currencyCode"])
    balance = float(account_funds["availableToBetBalance"])
    locked = -float(account_funds["exposure"]) if account_funds["exposure"] else 0.0
    free = balance - locked
    return AccountState(
        account_id=AccountId(f"{BETFAIR_VENUE.value}-{account_id}"),
        account_type=AccountType.BETTING,
        base_currency=currency,
        reported=False,
        balances=[
            AccountBalance(
                total=Money(balance, currency),
                locked=Money(locked, currency),
                free=Money(free, currency),
            ),
        ],
        margins=[],
        info={"funds": account_funds, "detail": account_detail},
        event_id=event_id,
        ts_event=ts_event,
        ts_init=ts_init,
    )


async def generate_trades_list(
    self,
    venue_order_id: VenueOrderId,
    symbol: Symbol,
    since: datetime = None,  # type: ignore
) -> list[TradeReport]:
    filled = self.client().betting.list_cleared_orders(
        bet_ids=[venue_order_id],
    )
    if not filled["clearedOrders"]:
        self._log.warn(f"Found no existing order for {venue_order_id}")
        return []
    fill = filled["clearedOrders"][0]
    ts_event = int(pd.Timestamp(fill["lastMatchedDate"]).to_datetime64())
    return [
        TradeReport(
            client_order_id=self.venue_order_id_to_client_order_id[venue_order_id],
            venue_order_id=VenueOrderId(fill["betId"]),
            venue_position_id=None,  # Can be None
            trade_id=TradeId(fill["lastMatchedDate"]),
            last_qty=Quantity.from_str(str(fill["sizeSettled"])),  # TODO: Incorrect precision?
            last_px=Price.from_str(str(fill["priceMatched"])),  # TODO: Incorrect precision?
            commission=None,  # Can be None
            liquidity_side=LiquiditySide.NONE,
            ts_event=ts_event,
            ts_init=ts_event,
        ),
    ]


@lru_cache(None)
def parse_handicap(x) -> Optional[str]:
    """
    Ensure consistent parsing of the various handicap sources we get.
    """
    if x in (None, ""):
        return "0.0"
    if isinstance(x, (int, str)):
        return str(float(x))
    elif isinstance(x, float):
        return str(x)
    else:
        raise TypeError(f"Unexpected type ({type(x)}) for handicap: {x}")


def bet_to_order_status_report(
    order,
    account_id: AccountId,
    instrument_id: InstrumentId,
    venue_order_id: VenueOrderId,
    client_order_id: ClientOrderId,
    ts_init,
    report_id,
) -> OrderStatusReport:
    return OrderStatusReport(
        account_id=account_id,
        instrument_id=instrument_id,
        venue_order_id=venue_order_id,
        client_order_id=client_order_id,
        order_side=B2N_ORDER_STREAM_SIDE[order["side"]],
        order_type=OrderTypeParser.from_str_py(order["orderType"]),
        contingency_type=ContingencyType.NONE,
        time_in_force=B2N_TIME_IN_FORCE[order["persistenceType"]],
        order_status=determine_order_status(order),
        price=price_to_probability(str(order["priceSize"]["price"])),
        quantity=Quantity(order["priceSize"]["size"], BETFAIR_QUANTITY_PRECISION),
        filled_qty=Quantity(order["sizeMatched"], BETFAIR_QUANTITY_PRECISION),
        report_id=report_id,
        ts_accepted=dt_to_unix_nanos(pd.Timestamp(order["placedDate"])),
        ts_triggered=0,
        ts_last=dt_to_unix_nanos(pd.Timestamp(order["matchedDate"]))
        if "matchedDate" in order
        else 0,
        ts_init=ts_init,
    )


def determine_order_status(order: dict) -> OrderStatus:
    order_size = order["priceSize"]["size"]
    if order["status"] == "EXECUTION_COMPLETE":
        if order_size == order["sizeMatched"]:
            return OrderStatus.FILLED
        elif order["sizeCancelled"] > 0.0:
            return OrderStatus.CANCELED
        else:
            return OrderStatus.PARTIALLY_FILLED
    elif order["status"] == "EXECUTABLE":
        if order["sizeMatched"] == 0.0:
            return OrderStatus.ACCEPTED
        elif order["sizeMatched"] > 0.0:
            return OrderStatus.PARTIALLY_FILLED
