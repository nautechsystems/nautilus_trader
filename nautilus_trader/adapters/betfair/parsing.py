# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from decimal import Decimal
import itertools
from typing import List, Optional, Union

from betfairlightweight.filters import cancel_instruction
from betfairlightweight.filters import limit_order
from betfairlightweight.filters import place_instruction
from betfairlightweight.filters import replace_instruction
import pandas as pd

from nautilus_trader.adapters.betfair.common import B2N_MARKET_STREAM_SIDE
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.common import B_ASK_KINDS
from nautilus_trader.adapters.betfair.common import B_BID_KINDS
from nautilus_trader.adapters.betfair.common import B_SIDE_KINDS
from nautilus_trader.adapters.betfair.common import N2B_SIDE
from nautilus_trader.adapters.betfair.common import N2B_TIME_IN_FORCE
from nautilus_trader.adapters.betfair.common import price_to_probability
from nautilus_trader.adapters.betfair.common import probability_to_price
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.util import hash_json
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevel
from nautilus_trader.model.c_enums.orderbook_op import OrderBookOperationType
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.commands import UpdateOrder
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderState
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instrument import BettingInstrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order.limit import LimitOrder
from nautilus_trader.model.orderbook.book import OrderBookOperation
from nautilus_trader.model.orderbook.book import OrderBookOperations
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.orderbook.order import Order
from nautilus_trader.model.tick import TradeTick


def order_submit_to_betfair(command: SubmitOrder, instrument: BettingInstrument):
    """ Convert a SubmitOrder command into the data required by betfairlightweight """

    order = command.order  # type: LimitOrder
    return {
        "market_id": instrument.market_id,
        # Used to de-dupe orders on betfair server side
        "customer_ref": command.id.value,
        "customer_strategy_ref": command.strategy_id.value,
        "instructions": [
            place_instruction(
                order_type="LIMIT",
                selection_id=instrument.selection_id,
                side=N2B_SIDE[order.side],
                handicap={"0.0": "0"}.get(
                    instrument.selection_handicap, instrument.selection_handicap
                ),
                limit_order=limit_order(
                    size=float(order.quantity),
                    price=float(
                        probability_to_price(probability=order.price, side=order.side)
                    ),
                    persistence_type="PERSIST",
                    time_in_force=N2B_TIME_IN_FORCE[order.time_in_force],
                    min_fill_size=0,
                ),
                customer_order_ref=order.client_order_id.value.replace("-", ""),
            )
        ],
    }


def order_update_to_betfair(
    command: UpdateOrder,
    venue_order_id: VenueOrderId,
    side: OrderSide,
    instrument: BettingInstrument,
):
    """ Convert an UpdateOrder command into the data required by betfairlightweight """
    return {
        "market_id": instrument.market_id,
        "customer_ref": str(command.id),
        "instructions": [
            replace_instruction(
                bet_id=venue_order_id.value,
                new_price=float(
                    probability_to_price(probability=command.price, side=side)
                ),
            )
        ],
    }


def order_cancel_to_betfair(command: CancelOrder, instrument: BettingInstrument):
    """ Convert a SubmitOrder command into the data required by betfairlightweight """
    return {
        "market_id": instrument.market_id,
        "customer_ref": command.id.value,
        "instructions": [cancel_instruction(bet_id=command.venue_order_id.value)],
    }


def betfair_account_to_account_state(
    account_detail,
    account_funds,
    event_id,
    timestamp_ns,
    account_id="001",
) -> AccountState:
    currency = Currency.from_str(account_detail["currencyCode"])
    balance = float(account_funds["availableToBetBalance"])
    balance_locked = -float(account_funds["exposure"])
    balance_free = balance - balance_locked
    return AccountState(
        AccountId(issuer=BETFAIR_VENUE.value, identifier=account_id),
        [Money(value=balance, currency=currency)],
        [Money(value=balance_free, currency=currency)],
        [Money(value=balance_locked, currency=currency)],
        {"funds": account_funds, "detail": account_detail},
        event_id,
        timestamp_ns,
    )


def build_market_snapshot_messages(
    raw, instrument_provider: BetfairInstrumentProvider
) -> List[OrderBookSnapshot]:
    updates = []
    for market in raw.get("mc", []):
        # Market status events
        # market_definition = market.get("marketDefinition", {})
        # TODO - Need to handle instrument status = CLOSED here

        # Orderbook snapshots
        if market.get("img") is True:
            market_id = market["id"]
            for (selection_id, handicap), selections in itertools.groupby(
                market.get("rc", []), lambda x: (x["id"], x.get("hc"))
            ):
                for selection in list(selections):
                    kw = dict(
                        market_id=market_id,
                        selection_id=str(selection_id),
                        handicap=str(handicap or "0.0"),
                    )
                    instrument = instrument_provider.get_betting_instrument(**kw)
                    # Check we only have one of [best bets / depth bets / all bets]
                    bid_keys = [k for k in B_BID_KINDS if k in selection] or ["atb"]
                    ask_keys = [k for k in B_ASK_KINDS if k in selection] or ["atl"]
                    assert len(bid_keys) <= 1
                    assert len(ask_keys) <= 1
                    # TODO Clean this crap up
                    if bid_keys[0] == "atb":
                        bids = selection.get("atb", [])
                    else:
                        bids = [(p, v) for _, p, v in selection.get(bid_keys[0], [])]
                    if ask_keys[0] == "atl":
                        asks = selection.get("atl", [])
                    else:
                        asks = [(p, v) for _, p, v in selection.get(ask_keys[0], [])]
                    snapshot = OrderBookSnapshot(
                        level=OrderBookLevel.L2,
                        instrument_id=instrument.id,
                        bids=[
                            (price_to_probability(p, OrderSide.BUY), v) for p, v in asks
                        ],
                        asks=[
                            (price_to_probability(p, OrderSide.SELL), v)
                            for p, v in bids
                        ],
                        timestamp_ns=millis_to_nanos(raw["pt"]),
                    )
                    updates.append(snapshot)
    return updates


def build_market_update_messages(  # noqa TODO: cyclomatic complexity 14
    raw, instrument_provider: BetfairInstrumentProvider
) -> List[Union[OrderBookOperation, TradeTick]]:
    updates = []
    for market in raw.get("mc", []):
        market_id = market["id"]
        for runner in market.get("rc", []):
            kw = dict(
                market_id=market_id,
                selection_id=str(runner["id"]),
                handicap=str(runner.get("hc") or "0.0"),
            )
            instrument = instrument_provider.get_betting_instrument(**kw)
            if not instrument:
                continue
            operations = []
            for side in B_SIDE_KINDS:
                for upd in runner.get(side, []):
                    # TODO - Fix this crap
                    if len(upd) == 3:
                        _, price, volume = upd
                    else:
                        price, volume = upd
                    operations.append(
                        OrderBookOperation(
                            op_type=OrderBookOperationType.DELETE
                            if volume == 0
                            else OrderBookOperationType.UPDATE,
                            order=Order(
                                price=price_to_probability(
                                    price, side=B2N_MARKET_STREAM_SIDE[side]
                                ),
                                volume=volume,
                                side=B2N_MARKET_STREAM_SIDE[side],
                            ),
                            timestamp_ns=millis_to_nanos(raw["pt"]),
                        )
                    )
            ob_update = OrderBookOperations(
                level=OrderBookLevel.L2,
                instrument_id=instrument.id,
                ops=operations,
                timestamp_ns=millis_to_nanos(raw["pt"]),
            )
            updates.append(ob_update)

            for price, volume in runner.get("trd", []):
                # Betfair doesn't publish trade ids, so we make our own
                # TODO - should we use clk here?
                trade_id = hash_json(
                    data=(
                        raw["pt"],
                        market_id,
                        str(runner["id"]),
                        str(runner.get("hc", "0.0")),
                        price,
                        volume,
                    )
                )
                trade_tick = TradeTick(
                    instrument_id=instrument.id,
                    price=Price(price_to_probability(price)),
                    size=Quantity(volume, precision=4),
                    side=OrderSide.BUY,
                    match_id=TradeMatchId(trade_id),
                    timestamp_ns=millis_to_nanos(raw["pt"]),
                )
                updates.append(trade_tick)

        if market.get("marketDefinition", {}).get("status") == "CLOSED":
            for runner in market["marketDefinition"]["runners"]:
                kw = dict(
                    market_id=market_id,
                    selection_id=str(runner["id"]),
                    handicap=str(runner.get("hc") or "0.0"),
                )
                instrument = instrument_provider.get_betting_instrument(**kw)
                assert instrument
                # TODO - handle market closed
                # on_market_status()

                if runner["status"] == "LOSER":
                    # TODO - handle closing valuation = 0
                    pass
                elif runner["status"] == "WINNER":
                    # TODO handle closing valuation = 1
                    pass
        if (
            market.get("marketDefinition", {}).get("inPlay")
            and not market.get("marketDefinition", {}).get("status") == "CLOSED"
        ):
            for selection in market["marketDefinition"]["runners"]:
                kw = dict(
                    market_id=market_id,
                    selection_id=str(selection["id"]),
                    handicap=str(
                        selection.get("hc", selection.get("handicap")) or "0.0"
                    ),
                )
                instrument = instrument_provider.get_betting_instrument(**kw)
                assert instrument
                # TODO - handle instrument status IN_PLAY
    return updates


def on_market_update(update: dict, instrument_provider: BetfairInstrumentProvider):
    if update.get("ct") == "HEARTBEAT":
        # TODO - Do we send out heartbeats
        return []
    for mc in update.get("mc", []):
        if mc.get("img"):
            return build_market_snapshot_messages(
                update, instrument_provider=instrument_provider
            )
        else:
            return build_market_update_messages(
                update, instrument_provider=instrument_provider
            )
    return []


# TODO - Need to handle pagination > 1000 orders
async def generate_order_status_report(self, order) -> Optional[OrderStatusReport]:
    return [
        OrderStatusReport(
            client_order_id=ClientOrderId(),
            venue_order_id=VenueOrderId(),
            order_state=OrderState(),
            filled_qty=Quantity(),
            timestamp_ns=millis_to_nanos(),
        )
        for order in self.client().betting.list_current_orders()["currentOrders"]
    ]


async def generate_trades_list(
    self, venue_order_id: VenueOrderId, symbol: Symbol, since: datetime = None
) -> List[ExecutionReport]:
    filled = self.client().betting.list_cleared_orders(
        bet_ids=[venue_order_id],
    )
    if not filled["clearedOrders"]:
        self._log.warn(f"Found no existing order for {venue_order_id}")
        return []
    fill = filled["clearedOrders"][0]
    timestamp_ns = millis_to_nanos(pd.Timestamp(fill["lastMatchedDate"]).timestamp())
    return [
        ExecutionReport(
            client_order_id=self.venue_order_id_to_client_order_id[venue_order_id],
            venue_order_id=VenueOrderId(fill["betId"]),
            execution_id=ExecutionId(fill["lastMatchedDate"]),
            last_qty=Decimal(fill["sizeSettled"]),
            last_px=Decimal(fill["priceMatched"]),
            commission_amount=None,  # Can be None
            commission_currency=None,  # Can be None
            liquidity_side=LiquiditySide.NONE,
            execution_ns=timestamp_ns,
            timestamp_ns=timestamp_ns,
        )
    ]
