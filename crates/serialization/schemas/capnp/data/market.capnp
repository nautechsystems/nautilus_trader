@0xe9ad557eba0125dc;
# Cap'n Proto schema for Nautilus market data types
#
# WARNING: This schema is not yet stable and may change without notice
# between releases. Do not depend on wire compatibility across versions.

using Identifiers = import "../common/identifiers.capnp";
using Types = import "../common/types.capnp";
using Enums = import "../common/enums.capnp";
using Base = import "../common/base.capnp";

# Quote tick - top-of-book bid/ask
struct QuoteTick {
    instrumentId @0 :Identifiers.InstrumentId;
    bidPrice @1 :Types.Price;
    askPrice @2 :Types.Price;
    bidSize @3 :Types.Quantity;
    askSize @4 :Types.Quantity;
    tsEvent @5 :Base.UnixNanos;
    tsInit @6 :Base.UnixNanos;
}

# Trade tick - individual trade
struct TradeTick {
    instrumentId @0 :Identifiers.InstrumentId;
    price @1 :Types.Price;
    size @2 :Types.Quantity;
    aggressorSide @3 :Enums.AggressorSide;
    tradeId @4 :Identifiers.TradeId;
    tsEvent @5 :Base.UnixNanos;
    tsInit @6 :Base.UnixNanos;
}

# Bar specification
struct BarSpec {
    step @0 :UInt32;
    aggregation @1 :Enums.BarAggregation;
    priceType @2 :Enums.PriceType;
}

# Bar type
struct BarType {
    instrumentId @0 :Identifiers.InstrumentId;
    spec @1 :BarSpec;
    aggregationSource @2 :Enums.AggregationSource;
}

# Bar/Candlestick
struct Bar {
    barType @0 :BarType;
    open @1 :Types.Price;
    high @2 :Types.Price;
    low @3 :Types.Price;
    close @4 :Types.Price;
    volume @5 :Types.Quantity;
    tsEvent @6 :Base.UnixNanos;
    tsInit @7 :Base.UnixNanos;
}

# Mark price update
struct MarkPriceUpdate {
    instrumentId @0 :Identifiers.InstrumentId;
    markPrice @1 :Types.Price;
    tsEvent @2 :Base.UnixNanos;
    tsInit @3 :Base.UnixNanos;
}

# Index price update
struct IndexPriceUpdate {
    instrumentId @0 :Identifiers.InstrumentId;
    indexPrice @1 :Types.Price;
    tsEvent @2 :Base.UnixNanos;
    tsInit @3 :Base.UnixNanos;
}

# Instrument close
struct InstrumentClose {
    instrumentId @0 :Identifiers.InstrumentId;
    closePrice @1 :Types.Price;
    closeType @2 :Enums.InstrumentCloseType;
    tsEvent @3 :Base.UnixNanos;
    tsInit @4 :Base.UnixNanos;
}

struct InstrumentStatus {
    instrumentId @0 :Identifiers.InstrumentId;
    action @1 :Enums.MarketStatusAction;
    reason @2 :Text;  # Optional - absence means None
    tradingEvent @3 :Text;  # Optional - absence means None
    isTrading @4 :Enums.OptionalBool;
    isQuoting @5 :Enums.OptionalBool;
    isShortSellRestricted @6 :Enums.OptionalBool;
    tsEvent @7 :Base.UnixNanos;
    tsInit @8 :Base.UnixNanos;
}

# Funding rate update
struct FundingRateUpdate {
    instrumentId @0 :Identifiers.InstrumentId;
    rate @1 :Types.Decimal;  # Decimal as binary (optimized)
    interval @2 :UInt16;  # Valid when hasInterval is true
    nextFundingTime @3 :Base.UnixNanos;  # Optional - absence means None
    tsEvent @4 :Base.UnixNanos;
    tsInit @5 :Base.UnixNanos;
    hasInterval @6 :Bool;  # Presence flag for interval
}

# Market data enum union
struct DataAny {
    union {
        quote @0 :QuoteTick;
        trade @1 :TradeTick;
        bar @2 :Bar;
        markPrice @3 :MarkPriceUpdate;
        indexPrice @4 :IndexPriceUpdate;
        instrumentClose @5 :InstrumentClose;
        instrumentStatus @6 :InstrumentStatus;
        fundingRate @7 :FundingRateUpdate;
        orderBookDelta @8 :OrderBookDelta;
        orderBookDeltas @9 :OrderBookDeltas;
        orderBookDepth10 @10 :OrderBookDepth10;
    }
}

# Book order
struct BookOrder {
    price @0 :Types.Price;
    size @1 :Types.Quantity;
    side @2 :Enums.OrderSide;
    orderId @3 :UInt64;
}

# Order book delta (single)
struct OrderBookDelta {
    instrumentId @0 :Identifiers.InstrumentId;
    action @1 :Enums.BookAction;
    order @2 :BookOrder;
    flags @3 :UInt8;
    sequence @4 :UInt64;
    tsEvent @5 :Base.UnixNanos;
    tsInit @6 :Base.UnixNanos;
}

# Order book deltas (batch)
struct OrderBookDeltas {
    instrumentId @0 :Identifiers.InstrumentId;
    deltas @1 :List(OrderBookDelta);
    flags @2 :UInt8;
    sequence @3 :UInt64;
    tsEvent @4 :Base.UnixNanos;
    tsInit @5 :Base.UnixNanos;
}

# Book level
struct BookLevel {
    price @0 :Types.Price;
    size @1 :Types.Quantity;
}

# Order book depth (top 10 levels)
struct OrderBookDepth10 {
    instrumentId @0 :Identifiers.InstrumentId;
    bids @1 :List(BookLevel);  # Up to 10 levels
    asks @2 :List(BookLevel);  # Up to 10 levels
    bidCounts @3 :List(UInt32);
    askCounts @4 :List(UInt32);
    flags @5 :UInt8;
    sequence @6 :UInt64;
    tsEvent @7 :Base.UnixNanos;
    tsInit @8 :Base.UnixNanos;
}
