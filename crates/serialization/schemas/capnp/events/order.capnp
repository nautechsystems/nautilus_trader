@0xd4e5f60718293041;
# Cap'n Proto schema for Nautilus order events

using Identifiers = import "../common/identifiers.capnp";
using Types = import "../common/types.capnp";
using Enums = import "../common/enums.capnp";
using Base = import "../common/base.capnp";

# Common header for all order events
struct OrderEventHeader {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    eventId @4 :Base.UUID4;
    tsInit @5 :Base.UnixNanos;
}

# Order event union
struct OrderEvent {
    union {
        initialized @0 :OrderInitialized;
        denied @1 :OrderDenied;
        emulated @2 :OrderEmulated;
        released @3 :OrderReleased;
        submitted @4 :OrderSubmitted;
        accepted @5 :OrderAccepted;
        rejected @6 :OrderRejected;
        canceled @7 :OrderCanceled;
        expired @8 :OrderExpired;
        triggered @9 :OrderTriggered;
        pendingUpdate @10 :OrderPendingUpdate;
        pendingCancel @11 :OrderPendingCancel;
        modifyRejected @12 :OrderModifyRejected;
        cancelRejected @13 :OrderCancelRejected;
        updated @14 :OrderUpdated;
        filled @15 :OrderFilled;
    }
}

# OrderInitialized - seed event that can instantiate any order
struct OrderInitialized {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    orderSide @4 :Enums.OrderSide;
    orderType @5 :Enums.OrderType;
    quantity @6 :Types.Quantity;
    timeInForce @7 :Enums.TimeInForce;
    postOnly @8 :Bool;
    reduceOnly @9 :Bool;
    quoteQuantity @10 :Bool;
    reconciliation @11 :Bool;
    eventId @12 :Base.UUID4;
    tsEvent @13 :Base.UnixNanos;
    tsInit @14 :Base.UnixNanos;
    price @15 :Types.Price;
    triggerPrice @16 :Types.Price;
    triggerType @17 :Enums.TriggerType;
    limitOffset @18 :Types.Decimal;
    trailingOffset @19 :Types.Decimal;
    trailingOffsetType @20 :Enums.TrailingOffsetType;
    expireTime @21 :Base.UnixNanos;
    displayQty @22 :Types.Quantity;
    emulationTrigger @23 :Enums.TriggerType;
    triggerInstrumentId @24 :Identifiers.InstrumentId;
    contingencyType @25 :Enums.ContingencyType;
    orderListId @26 :Identifiers.OrderListId;
    linkedOrderIds @27 :List(Identifiers.ClientOrderId);
    parentOrderId @28 :Identifiers.ClientOrderId;
    execAlgorithmId @29 :Identifiers.ExecAlgorithmId;
    execAlgorithmParams @30 :Base.StringMap;
    execSpawnId @31 :Identifiers.ClientOrderId;
    tags @32 :List(Text);
}

# OrderDenied - order denied by system
struct OrderDenied {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    reason @4 :Text;
    eventId @5 :Base.UUID4;
    tsInit @6 :Base.UnixNanos;
}

# OrderEmulated - order held and managed by the Nautilus system
struct OrderEmulated {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    eventId @4 :Base.UUID4;
    tsInit @5 :Base.UnixNanos;
}

# OrderReleased - emulated order released to the market
struct OrderReleased {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    releasedPrice @4 :Types.Price;
    eventId @5 :Base.UUID4;
    tsInit @6 :Base.UnixNanos;
}

# OrderSubmitted - order submitted to the venue
struct OrderSubmitted {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    accountId @4 :Identifiers.AccountId;
    eventId @5 :Base.UUID4;
    tsEvent @6 :Base.UnixNanos;
    tsInit @7 :Base.UnixNanos;
}

# OrderAccepted - order accepted by the venue
struct OrderAccepted {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    venueOrderId @4 :Identifiers.VenueOrderId;
    accountId @5 :Identifiers.AccountId;
    eventId @6 :Base.UUID4;
    tsEvent @7 :Base.UnixNanos;
    tsInit @8 :Base.UnixNanos;
    reconciliation @9 :Bool;
}

# OrderRejected - order rejected by the venue
struct OrderRejected {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    accountId @4 :Identifiers.AccountId;
    reason @5 :Text;
    eventId @6 :Base.UUID4;
    tsEvent @7 :Base.UnixNanos;
    tsInit @8 :Base.UnixNanos;
    reconciliation @9 :Bool;
    duePostOnly @10 :Bool;
}

# OrderCanceled - order canceled
struct OrderCanceled {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    venueOrderId @4 :Identifiers.VenueOrderId;
    accountId @5 :Identifiers.AccountId;
    eventId @6 :Base.UUID4;
    tsEvent @7 :Base.UnixNanos;
    tsInit @8 :Base.UnixNanos;
    reconciliation @9 :Bool;
}

# OrderExpired - order expired
struct OrderExpired {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    venueOrderId @4 :Identifiers.VenueOrderId;
    accountId @5 :Identifiers.AccountId;
    eventId @6 :Base.UUID4;
    tsEvent @7 :Base.UnixNanos;
    tsInit @8 :Base.UnixNanos;
    reconciliation @9 :Bool;
}

# OrderTriggered - stop order triggered
struct OrderTriggered {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    venueOrderId @4 :Identifiers.VenueOrderId;
    accountId @5 :Identifiers.AccountId;
    eventId @6 :Base.UUID4;
    tsEvent @7 :Base.UnixNanos;
    tsInit @8 :Base.UnixNanos;
    reconciliation @9 :Bool;
}

# OrderPendingUpdate - order modify request pending
struct OrderPendingUpdate {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    venueOrderId @4 :Identifiers.VenueOrderId;
    accountId @5 :Identifiers.AccountId;
    eventId @6 :Base.UUID4;
    tsEvent @7 :Base.UnixNanos;
    tsInit @8 :Base.UnixNanos;
    reconciliation @9 :Bool;
}

# OrderPendingCancel - order cancel request pending
struct OrderPendingCancel {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    venueOrderId @4 :Identifiers.VenueOrderId;
    accountId @5 :Identifiers.AccountId;
    eventId @6 :Base.UUID4;
    tsEvent @7 :Base.UnixNanos;
    tsInit @8 :Base.UnixNanos;
    reconciliation @9 :Bool;
}

# OrderModifyRejected - order modify request rejected
struct OrderModifyRejected {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    venueOrderId @4 :Identifiers.VenueOrderId;
    accountId @5 :Identifiers.AccountId;
    reason @6 :Text;
    eventId @7 :Base.UUID4;
    tsEvent @8 :Base.UnixNanos;
    tsInit @9 :Base.UnixNanos;
    reconciliation @10 :Bool;
}

# OrderCancelRejected - order cancel request rejected
struct OrderCancelRejected {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    venueOrderId @4 :Identifiers.VenueOrderId;
    accountId @5 :Identifiers.AccountId;
    reason @6 :Text;
    eventId @7 :Base.UUID4;
    tsEvent @8 :Base.UnixNanos;
    tsInit @9 :Base.UnixNanos;
    reconciliation @10 :Bool;
}

# OrderUpdated - order parameters updated
struct OrderUpdated {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    venueOrderId @4 :Identifiers.VenueOrderId;
    accountId @5 :Identifiers.AccountId;
    quantity @6 :Types.Quantity;
    price @7 :Types.Price;
    triggerPrice @8 :Types.Price;
    protectionPrice @9 :Types.Price;  # Optional
    eventId @10 :Base.UUID4;
    tsEvent @11 :Base.UnixNanos;
    tsInit @12 :Base.UnixNanos;
    reconciliation @13 :Bool;
}

# OrderFilled - order fill execution
struct OrderFilled {
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    venueOrderId @4 :Identifiers.VenueOrderId;
    accountId @5 :Identifiers.AccountId;
    tradeId @6 :Identifiers.TradeId;
    orderSide @7 :Enums.OrderSide;
    orderType @8 :Enums.OrderType;
    lastQty @9 :Types.Quantity;
    lastPx @10 :Types.Price;
    currency @11 :Types.Currency;
    liquiditySide @12 :Enums.LiquiditySide;
    eventId @13 :Base.UUID4;
    tsEvent @14 :Base.UnixNanos;
    tsInit @15 :Base.UnixNanos;
    reconciliation @16 :Bool;
    positionId @17 :Identifiers.PositionId;
    commission @18 :Types.Money;
}
