@0xfaed26c32ecd3500;
# Cap'n Proto schema for Nautilus position events
#
# Design Note: Float64 Optimization Fields
# Position events include both fixed-point types (Types.Quantity, Types.Price) and
# Float64 fields (signedQty, avgPxOpen, avgPxClose, realizedReturn). This redundancy
# is intentional for performance:
#
# - Fixed-point fields: Precise storage matching exchange-reported values
# - Float64 fields: Optimized calculation fields to avoid fixed-point arithmetic
#   in hot paths. Consumers can use these directly for performance-critical calculations.
#
# The Float64 values are derived from fixed-point values during event creation.
# Trade-off: ~16-32 extra bytes per message for faster processing.

using Identifiers = import "../common/identifiers.capnp";
using Types = import "../common/types.capnp";
using Enums = import "../common/enums.capnp";
using Base = import "../common/base.capnp";

# Common header for all position events
struct PositionEventHeader {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    positionId @3 :Identifiers.PositionId;
    accountId @4 :Identifiers.AccountId;
    openingOrderId @5 :Identifiers.ClientOrderId;
    entry @6 :Enums.OrderSide;
    side @7 :Enums.PositionSide;
    signedQty @8 :Float64;
    quantity @9 :Types.Quantity;
    eventId @10 :Base.UUID4;
    tsInit @11 :Base.UnixNanos;
}

struct PositionEvent {
    union {
        opened @0 :PositionOpened;
        changed @1 :PositionChanged;
        closed @2 :PositionClosed;
        adjusted @3 :PositionAdjusted;
    }
}

struct PositionOpened {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    positionId @3 :Identifiers.PositionId;
    accountId @4 :Identifiers.AccountId;
    openingOrderId @5 :Identifiers.ClientOrderId;
    entry @6 :Enums.OrderSide;
    side @7 :Enums.PositionSide;
    signedQty @8 :Float64;
    quantity @9 :Types.Quantity;
    lastQty @10 :Types.Quantity;
    lastPx @11 :Types.Price;
    currency @12 :Types.Currency;
    avgPxOpen @13 :Float64;
    eventId @14 :Base.UUID4;
    tsEvent @15 :Base.UnixNanos;
    tsInit @16 :Base.UnixNanos;
}

struct PositionChanged {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    positionId @3 :Identifiers.PositionId;
    accountId @4 :Identifiers.AccountId;
    openingOrderId @5 :Identifiers.ClientOrderId;
    entry @6 :Enums.OrderSide;
    side @7 :Enums.PositionSide;
    signedQty @8 :Float64;
    quantity @9 :Types.Quantity;
    peakQuantity @10 :Types.Quantity;
    lastQty @11 :Types.Quantity;
    lastPx @12 :Types.Price;
    currency @13 :Types.Currency;
    avgPxOpen @14 :Float64;
    avgPxClose @15 :Float64;
    realizedReturn @16 :Float64;
    realizedPnl @17 :Types.Money;
    unrealizedPnl @18 :Types.Money;
    eventId @19 :Base.UUID4;
    tsOpened @20 :Base.UnixNanos;
    tsEvent @21 :Base.UnixNanos;
    tsInit @22 :Base.UnixNanos;
}

struct PositionClosed {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    positionId @3 :Identifiers.PositionId;
    accountId @4 :Identifiers.AccountId;
    openingOrderId @5 :Identifiers.ClientOrderId;
    closingOrderId @6 :Identifiers.ClientOrderId;
    entry @7 :Enums.OrderSide;
    side @8 :Enums.PositionSide;
    signedQty @9 :Float64;
    quantity @10 :Types.Quantity;
    peakQuantity @11 :Types.Quantity;
    lastQty @12 :Types.Quantity;
    lastPx @13 :Types.Price;
    currency @14 :Types.Currency;
    avgPxOpen @15 :Float64;
    avgPxClose @16 :Float64;
    realizedReturn @17 :Float64;
    realizedPnl @18 :Types.Money;
    unrealizedPnl @19 :Types.Money;
    duration @20 :UInt64;  # Duration in nanoseconds
    eventId @21 :Base.UUID4;
    tsOpened @22 :Base.UnixNanos;
    tsClosed @23 :Base.UnixNanos;
    tsEvent @24 :Base.UnixNanos;
    tsInit @25 :Base.UnixNanos;
}

# Simplified position adjustment event
struct PositionAdjusted {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    positionId @3 :Identifiers.PositionId;
    accountId @4 :Identifiers.AccountId;
    adjustmentType @5 :Enums.PositionAdjustmentType;
    quantityChange @6 :Types.Decimal;  # Optional - check if all fields are 0
    pnlChange @7 :Types.Money;         # Optional
    reason @8 :Text;                   # Optional - empty string means None
    eventId @9 :Base.UUID4;
    tsEvent @10 :Base.UnixNanos;
    tsInit @11 :Base.UnixNanos;
}
