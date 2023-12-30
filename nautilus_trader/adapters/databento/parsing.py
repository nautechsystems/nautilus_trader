# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import databento
import pandas as pd
import pytz

from nautilus_trader.adapters.databento.common import nautilus_instrument_id_from_databento
from nautilus_trader.adapters.databento.enums import DatabentoInstrumentClass
from nautilus_trader.adapters.databento.enums import DatabentoStatisticType
from nautilus_trader.adapters.databento.enums import DatabentoStatisticUpdateAction
from nautilus_trader.adapters.databento.types import DatabentoImbalance
from nautilus_trader.adapters.databento.types import DatabentoPublisher
from nautilus_trader.adapters.databento.types import DatabentoStatistics
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import InstrumentClass
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import OptionsContract
from nautilus_trader.model.objects import FIXED_SCALAR
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def parse_order_side(value: str) -> OrderSide:
    match value:
        case "A":
            return OrderSide.SELL
        case "B":
            return OrderSide.BUY
        case _:
            return OrderSide.NO_ORDER_SIDE


def parse_aggressor_side(value: str) -> AggressorSide:
    match value:
        case "A":
            return AggressorSide.SELLER
        case "B":
            return AggressorSide.BUYER
        case _:
            return AggressorSide.NO_AGGRESSOR


def parse_book_action(value: str) -> BookAction:
    match value:
        case "A":
            return BookAction.ADD
        case "C":
            return BookAction.DELETE
        case "F":
            return BookAction.UPDATE
        case "M":
            return BookAction.UPDATE
        case "R":
            return BookAction.CLEAR
        case _:
            raise ValueError(f"Invalid `BookAction`, was '{value}'")


def parse_option_kind(value: str) -> OptionKind:
    match value:
        case "C":
            return OptionKind.CALL
        case "P":
            return OptionKind.PUT
        case _:
            raise ValueError(f"Invalid `OptionKind`, was '{value}'")


def parse_cfi_iso10926(value: str) -> tuple[AssetClass | None, InstrumentClass | None]:
    # This is a work in progress and will likely result in a shuffling of
    # the `AssetClass` and `InstrumentClass` enums

    cfi_category = value[0]
    cfi_group = value[1]
    cfi_attribute1 = value[2]
    # cfi_attribute2 = value[3]
    # cfi_attribute3 = value[4]
    # cfi_attribute4 = value[5]

    match cfi_category:
        case "D":
            asset_class = AssetClass.BOND
        case "E":
            asset_class = AssetClass.EQUITY
        case "S":
            asset_class = None
            instrument_class = InstrumentClass.SWAP
        case "J":
            asset_class = None
            instrument_class = InstrumentClass.FORWARD
        case _:
            asset_class = None

    match cfi_group:
        case "I":
            instrument_class = InstrumentClass.FUTURE
        case _:
            instrument_class = None

    match cfi_attribute1:
        case "I":
            asset_class = AssetClass.INDEX

    return (asset_class, instrument_class)


def parse_min_price_increment(value: int, currency: Currency) -> Price:
    match value:
        case 0 | 9223372036854775807:  # 2**63-1 (TODO: Make limit constants)
            return Price(10 ** (-currency.precision), currency.precision)
        case _:
            return Price.from_raw(value, currency.precision)


def parse_equity(
    record: databento.InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: int,
) -> Equity:
    # Use USD for all US equities venues for now
    currency = USD

    return Equity(
        instrument_id=instrument_id,
        raw_symbol=Symbol(record.raw_symbol),
        currency=currency,
        price_precision=currency.precision,
        price_increment=parse_min_price_increment(record.min_price_increment, currency),
        multiplier=Quantity(1, precision=0),
        lot_size=Quantity(record.min_lot_size_round_lot, precision=0),
        isin=None,  # TODO
        ts_event=record.ts_recv,  # More accurate and reliable timestamp
        ts_init=ts_init,
    )


def parse_futures_contract(
    record: databento.InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: int,
) -> FuturesContract:
    currency = Currency.from_str(record.currency)
    asset_class, _ = parse_cfi_iso10926(record.cfi)

    return FuturesContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol(record.raw_symbol),
        asset_class=asset_class or AssetClass.COMMODITY,  # WIP
        currency=currency,
        price_precision=currency.precision,
        price_increment=parse_min_price_increment(record.min_price_increment, currency),
        multiplier=Quantity(1, precision=0),
        lot_size=Quantity(record.min_lot_size_round_lot or 1, precision=0),
        underlying=record.asset,
        activation_ns=record.activation,
        expiration_ns=record.expiration,
        ts_event=record.ts_recv,  # More accurate and reliable timestamp
        ts_init=ts_init,
    )


def parse_options_contract(
    record: databento.InstrumentDefMsg,
    instrument_id: InstrumentId,
    ts_init: int,
) -> OptionsContract:
    currency = Currency.from_str(record.currency)

    if instrument_id.venue.value == "OPRA":
        lot_size = Quantity(1, precision=0)
        asset_class = AssetClass.EQUITY
    else:
        lot_size = Quantity(record.min_lot_size_round_lot or 1, precision=0)
        asset_class, _ = parse_cfi_iso10926(record.cfi)
        asset_class = asset_class or AssetClass.EQUITY  # WIP

    return OptionsContract(
        instrument_id=instrument_id,
        raw_symbol=Symbol(record.raw_symbol),
        asset_class=asset_class,
        currency=currency,
        price_precision=currency.precision,
        price_increment=parse_min_price_increment(record.min_price_increment, currency),
        multiplier=Quantity(1, precision=0),
        lot_size=lot_size,
        underlying=record.underlying,
        kind=parse_option_kind(record.instrument_class),
        activation_ns=record.activation,
        expiration_ns=record.expiration,
        strike_price=Price.from_raw(record.strike_price, currency.precision),
        ts_event=record.ts_recv,  # More accurate and reliable timestamp
        ts_init=ts_init,
    )


def parse_mbo_msg(
    record: databento.MBOMsg,
    instrument_id: InstrumentId,
    ts_init: int,
) -> OrderBookDelta | TradeTick:
    side: OrderSide = parse_order_side(record.side)
    if side == OrderSide.NO_ORDER_SIDE or record.action == "T":
        return TradeTick.from_raw(
            instrument_id=instrument_id,
            price_raw=record.price,
            price_prec=USD.precision,  # TODO(per instrument precision)
            size_raw=int(record.size * FIXED_SCALAR),  # No fractional sizes
            size_prec=0,  # No fractional units
            aggressor_side=parse_aggressor_side(record.side),
            trade_id=TradeId(str(record.sequence)),
            ts_event=record.ts_recv,  # More accurate and reliable timestamp
            ts_init=ts_init,
        )

    return OrderBookDelta.from_raw(
        instrument_id=instrument_id,
        action=parse_book_action(record.action),
        side=side,
        price_raw=record.price,
        price_prec=USD.precision,  # TODO(per instrument precision)
        size_raw=int(record.size * FIXED_SCALAR),  # No fractional sizes
        size_prec=0,  # No fractional units
        order_id=record.order_id,
        flags=record.flags,
        sequence=record.sequence,
        ts_event=record.ts_recv,  # More accurate and reliable timestamp
        ts_init=ts_init,
    )


def parse_mbp1_msg(
    record: databento.MBP1Msg,
    instrument_id: InstrumentId,
    ts_init: int,
) -> QuoteTick | tuple[QuoteTick | TradeTick]:
    top_level = record.levels[0]
    quote = QuoteTick.from_raw(
        instrument_id=instrument_id,
        bid_price_raw=top_level.bid_px,
        bid_price_prec=USD.precision,  # TODO(per instrument precision)
        ask_price_raw=top_level.ask_px,
        ask_price_prec=USD.precision,  # TODO(per instrument precision)
        bid_size_raw=int(top_level.bid_sz * FIXED_SCALAR),  # No fractional sizes
        bid_size_prec=0,  # No fractional units
        ask_size_raw=int(top_level.ask_sz * FIXED_SCALAR),  # No fractional sizes
        ask_size_prec=0,  # No fractional units
        ts_event=record.ts_recv,  # More accurate and reliable timestamp
        ts_init=ts_init,
    )

    match record.action:
        case "T":
            trade = TradeTick.from_raw(
                instrument_id=instrument_id,
                price_raw=record.price,
                price_prec=USD.precision,  # TODO(per instrument precision)
                size_raw=int(record.size * FIXED_SCALAR),  # No fractional sizes
                size_prec=0,  # No fractional units
                aggressor_side=parse_aggressor_side(record.side),
                trade_id=TradeId(str(record.sequence)),
                ts_event=record.ts_recv,  # More accurate and reliable timestamp
                ts_init=ts_init,
            )
            return quote, trade
        case _:
            return quote


def parse_mbp10_msg(
    record: databento.MBP10Msg,
    instrument_id: InstrumentId,
    ts_init: int,
) -> OrderBookDeltas:
    bids: list[OrderBookDelta] = []
    asks: list[OrderBookDelta] = []

    for level in record.levels:
        bid = OrderBookDelta.from_raw(
            instrument_id=instrument_id,
            action=BookAction.ADD,
            side=OrderSide.BUY,
            price_raw=level.bid_px,
            price_prec=USD.precision,  # TODO(per instrument precision)
            size_raw=int(level.bid_sz * FIXED_SCALAR),  # No fractional sizes
            size_prec=0,  # No fractional units
            order_id=0,  # No order ID for MBP level
            flags=record.flags,
            sequence=record.sequence,
            ts_event=record.ts_recv,  # More accurate and reliable timestamp
            ts_init=ts_init,
        )
        bids.append(bid)

        ask = OrderBookDelta.from_raw(
            instrument_id=instrument_id,
            action=BookAction.ADD,
            side=OrderSide.SELL,
            price_raw=level.ask_px,
            price_prec=USD.precision,  # TODO(per instrument precision)
            size_raw=int(level.ask_sz * FIXED_SCALAR),  # No fractional sizes
            size_prec=0,  # No fractional units
            order_id=0,  # No order ID for MBP level
            flags=record.flags,
            sequence=record.sequence,
            ts_event=record.ts_recv,  # More accurate and reliable timestamp
            ts_init=ts_init,
        )
        asks.append(ask)

    clear = [OrderBookDelta.clear(instrument_id, record.ts_recv, record.ts_recv, record.sequence)]
    return OrderBookDeltas(instrument_id=instrument_id, deltas=clear + bids + asks)


def parse_trade_msg(
    record: databento.TradeMsg,
    instrument_id: InstrumentId,
    ts_init: int,
) -> TradeTick:
    return TradeTick.from_raw(
        instrument_id=instrument_id,
        price_raw=record.price,
        price_prec=USD.precision,  # TODO(per instrument precision)
        size_raw=int(record.size * FIXED_SCALAR),
        size_prec=0,  # No fractional units
        aggressor_side=parse_aggressor_side(record.side),
        trade_id=TradeId(str(record.sequence)),
        ts_event=record.ts_recv,  # More accurate and reliable timestamp
        ts_init=ts_init,
    )


def parse_ohlcv_msg(
    record: databento.OHLCVMsg,
    instrument_id: InstrumentId,
    ts_init: int,
) -> Bar:
    match record.rtype:
        case 32:  # ohlcv-1s
            bar_spec = BarSpecification(1, BarAggregation.SECOND, PriceType.LAST)
            bar_type = BarType(instrument_id, bar_spec, AggregationSource.EXTERNAL)
            ts_event_adjustment = secs_to_nanos(1)
        case 33:  # ohlcv-1m
            bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)
            bar_type = BarType(instrument_id, bar_spec, AggregationSource.EXTERNAL)
            ts_event_adjustment = secs_to_nanos(60)
        case 34:  # ohlcv-1h
            bar_spec = BarSpecification(1, BarAggregation.HOUR, PriceType.LAST)
            bar_type = BarType(instrument_id, bar_spec, AggregationSource.EXTERNAL)
            ts_event_adjustment = secs_to_nanos(60 * 60)
        case 35:  # ohlcv-1d
            bar_spec = BarSpecification(1, BarAggregation.DAY, PriceType.LAST)
            bar_type = BarType(instrument_id, bar_spec, AggregationSource.EXTERNAL)
            ts_event_adjustment = secs_to_nanos(60 * 60 * 24)
        case _:
            raise ValueError("`rtype` is not a supported bar aggregation")

    # Adjust `ts_event` from open to close of bar
    ts_event = record.ts_event + ts_event_adjustment
    ts_init = max(ts_init, ts_event)

    return Bar(
        bar_type=bar_type,
        open=Price.from_raw(record.open / 100, 2),  # TODO(adjust for display factor)
        high=Price.from_raw(record.high / 100, 2),  # TODO(adjust for display factor)
        low=Price.from_raw(record.low / 100, 2),  # TODO(adjust for display factor)
        close=Price.from_raw(record.close / 100, 2),  # TODO(adjust for display factor)
        volume=Quantity.from_raw(record.volume, 0),  # TODO(adjust for display factor)
        ts_event=ts_event,
        ts_init=ts_init,
    )


def parse_imbalance_msg(
    record: databento.ImbalanceMsg,
    instrument_id: InstrumentId,
    ts_init: int,
) -> TradeTick:
    return DatabentoImbalance(
        instrument_id=instrument_id,
        ref_price=Price.from_raw(record.ref_price, USD.precision),
        cont_book_clr_price=Price.from_raw(record.cont_book_clr_price, USD.precision),
        auct_interest_clr_price=Price.from_raw(record.auct_interest_clr_price, USD.precision),
        paired_qty=Quantity.from_int(record.paired_qty),  # Always ints for now
        total_imbalance_qty=Quantity.from_int(record.total_imbalance_qty),  # Always ints for now
        side=parse_order_side(record.side),
        significant_imbalance=record.significant_imbalance,
        ts_event=record.ts_recv,  # More accurate and reliable timestamp
        ts_init=ts_init,
    )


def parse_statistics_msg(
    record: databento.StatMsg,
    instrument_id: InstrumentId,
    ts_init: int,
) -> TradeTick:
    return DatabentoStatistics(
        instrument_id=instrument_id,
        stat_type=DatabentoStatisticType(record.stat_type),
        update_action=DatabentoStatisticUpdateAction(record.update_action),
        price=Price.from_raw(record.price, USD.precision)
        if record.price is not (2**63 - 1)  # TODO: Define a constant for this
        else None,
        quantity=Quantity.from_raw(record.quantity, USD.precision)
        if record.quantity is not (2**31 - 1)  # TODO: Define a constant for this
        else None,
        channel_id=record.channel_id,
        stat_flags=record.stat_flags,
        sequence=record.sequence,
        ts_ref=record.ts_ref,
        ts_in_delta=record.ts_in_delta,
        ts_event=record.ts_recv,  # More accurate and reliable timestamp
        ts_init=ts_init,
    )


def parse_record_with_metadata(
    record: databento.DBNRecord,
    publishers: dict[int, DatabentoPublisher],
    ts_init: int,
    instrument_map: databento.InstrumentMap | None = None,
) -> Data:
    if isinstance(record, databento.InstrumentDefMsg):
        return parse_instrument_def(record, publishers, ts_init)

    if instrument_map is None:
        raise ValueError("`instrument_map` was `None` when a value was expected")

    record_date = pd.Timestamp(record.ts_event, tz=pytz.utc).date()
    raw_symbol = instrument_map.resolve(record.instrument_id, date=record_date)
    if raw_symbol is None:
        raise ValueError(
            f"Cannot resolve instrument_id {record.instrument_id} on {record_date}",
        )

    publisher: DatabentoPublisher = publishers[record.publisher_id]
    instrument_id: InstrumentId = nautilus_instrument_id_from_databento(
        raw_symbol=raw_symbol,
        publisher=publisher,
    )

    return parse_record(
        record=record,
        instrument_id=instrument_id,
        ts_init=ts_init,
    )


def parse_record(
    record: databento.DBNRecord,
    instrument_id: InstrumentId,
    ts_init: int,
) -> Data:
    if isinstance(record, databento.MBOMsg):
        return parse_mbo_msg(record, instrument_id, ts_init)
    elif isinstance(record, databento.MBP1Msg):  # Also TBBO
        return parse_mbp1_msg(record, instrument_id, ts_init)
    elif isinstance(record, databento.MBP10Msg):
        return parse_mbp10_msg(record, instrument_id, ts_init)
    elif isinstance(record, databento.TradeMsg):
        return parse_trade_msg(record, instrument_id, ts_init)
    elif isinstance(record, databento.OHLCVMsg):
        return parse_ohlcv_msg(record, instrument_id, ts_init)
    elif isinstance(record, databento.ImbalanceMsg):
        return parse_imbalance_msg(record, instrument_id, ts_init)
    elif isinstance(record, databento.StatMsg):
        return parse_statistics_msg(record, instrument_id, ts_init)
    else:
        raise ValueError(
            f"Schema {type(record).__name__} is currently unsupported by NautilusTrader",
        )


def parse_instrument_def(
    record: databento.InstrumentDefMsg,
    publishers: dict[int, DatabentoPublisher],
    ts_init: int,
) -> Instrument:
    publisher: DatabentoPublisher = publishers[record.publisher_id]
    instrument_id: InstrumentId = nautilus_instrument_id_from_databento(
        raw_symbol=record.raw_symbol,
        publisher=publisher,
    )

    match record.instrument_class:
        case DatabentoInstrumentClass.STOCK.value:
            return parse_equity(record, instrument_id, ts_init)
        case DatabentoInstrumentClass.FUTURE.value | DatabentoInstrumentClass.FUTURE_SPREAD.value:
            return parse_futures_contract(record, instrument_id, ts_init)
        case DatabentoInstrumentClass.CALL.value | DatabentoInstrumentClass.PUT.value:
            return parse_options_contract(record, instrument_id, ts_init)
        case DatabentoInstrumentClass.FUTURE_SPREAD.value:
            raise ValueError("`instrument_class` FUTURE_SPREAD not currently supported")
        case DatabentoInstrumentClass.OPTION_SPREAD.value:
            raise ValueError("`instrument_class` OPTION_SPREAD not currently supported")
        case DatabentoInstrumentClass.MIXED_SPREAD.value:
            raise ValueError("`instrument_class` MIXED_SPREAD not currently supported")
        case DatabentoInstrumentClass.FX_SPOT.value:
            raise ValueError("`instrument_class` FX_SPOT not currently supported")
        case _:
            raise ValueError(f"Invalid `instrument_class`, was {record.instrument_class}")
