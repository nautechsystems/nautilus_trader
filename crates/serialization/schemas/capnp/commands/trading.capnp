@0xc3d4e5f607182930;
# Cap'n Proto schema for Nautilus trading commands

using Identifiers = import "../common/identifiers.capnp";
using Types = import "../common/types.capnp";
using Enums = import "../common/enums.capnp";
using Base = import "../common/base.capnp";
using OrderEvents = import "../events/order.capnp";

# Common header for trading commands
struct TradingCommandHeader {
    traderId @0 :Identifiers.TraderId;
    clientId @1 :Identifiers.ClientId;
    strategyId @2 :Identifiers.StrategyId;
    instrumentId @3 :Identifiers.InstrumentId;
    commandId @4 :Base.UUID4;
    tsInit @5 :Base.UnixNanos;
}

# Order snapshot - bag of values representing current order state
# Many fields are optional depending on order type
struct Order {
    # Core identification
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    clientOrderId @3 :Identifiers.ClientOrderId;
    venueOrderId @4 :Identifiers.VenueOrderId;  # Optional
    positionId @5 :Identifiers.PositionId;  # Optional
    accountId @6 :Identifiers.AccountId;  # Optional
    lastTradeId @7 :Identifiers.TradeId;  # Optional

    # Order classification
    side @8 :Enums.OrderSide;
    orderType @9 :Enums.OrderType;
    status @10 :Enums.OrderStatus;

    # Quantities
    quantity @11 :Types.Quantity;
    filledQty @12 :Types.Quantity;
    leavesQty @13 :Types.Quantity;

    # Execution
    timeInForce @14 :Enums.TimeInForce;
    liquiditySide @15 :Enums.LiquiditySide;  # Optional

    # Prices (optional depending on order type)
    price @16 :Types.Price;  # Optional - for limit orders
    triggerPrice @17 :Types.Price;  # Optional - for stop orders

    # Price metadata
    avgPx @18 :Float64;  # Optional
    slippage @19 :Float64;  # Optional

    # Flags
    isReduceOnly @20 :Bool;
    isQuoteQuantity @21 :Bool;

    # Advanced order parameters (optional)
    expireTime @22 :Base.UnixNanos;  # Optional - 0 means None
    displayQty @23 :Types.Quantity;  # Optional
    emulationTrigger @24 :Enums.TriggerType;  # Optional
    triggerInstrumentId @25 :Identifiers.InstrumentId;  # Optional

    # Contingency orders
    contingencyType @26 :Enums.ContingencyType;  # Optional
    orderListId @27 :Identifiers.OrderListId;  # Optional
    linkedOrderIds @28 :List(Identifiers.ClientOrderId);  # Optional
    parentOrderId @29 :Identifiers.ClientOrderId;  # Optional

    # Execution algorithm
    execAlgorithmId @30 :Identifiers.ExecAlgorithmId;  # Optional
    execAlgorithmParams @31 :Base.StringMap;  # Optional
    execSpawnId @32 :Identifiers.ClientOrderId;  # Optional

    # Trailing stop parameters (optional)
    trailingOffset @33 :Types.Decimal;  # Optional
    trailingOffsetType @34 :Enums.TrailingOffsetType;  # Optional
    limitOffset @35 :Types.Decimal;  # Optional
    triggerType @36 :Enums.TriggerType;  # Optional

    # Metadata
    tags @37 :List(Text);  # Optional

    # Timestamps
    initId @38 :Base.UUID4;
    tsInit @39 :Base.UnixNanos;
    tsSubmitted @40 :Base.UnixNanos;  # Optional - 0 means None
    tsAccepted @41 :Base.UnixNanos;  # Optional - 0 means None
    tsClosed @42 :Base.UnixNanos;  # Optional - 0 means None
    tsLast @43 :Base.UnixNanos;
}

# Position snapshot - bag of values representing current position state
struct Position {
    # Core identification
    traderId @0 :Identifiers.TraderId;
    strategyId @1 :Identifiers.StrategyId;
    instrumentId @2 :Identifiers.InstrumentId;
    id @3 :Identifiers.PositionId;
    accountId @4 :Identifiers.AccountId;

    # Opening/closing orders
    openingOrderId @5 :Identifiers.ClientOrderId;
    closingOrderId @6 :Identifiers.ClientOrderId;  # Optional

    # Position state
    entry @7 :Enums.OrderSide;
    side @8 :Enums.PositionSide;
    signedQty @9 :Float64;
    quantity @10 :Types.Quantity;
    peakQty @11 :Types.Quantity;

    # Instrument parameters
    pricePrecision @12 :UInt8;
    sizePrecision @13 :UInt8;
    multiplier @14 :Types.Quantity;
    isInverse @15 :Bool;
    isCurrencyPair @16 :Bool;
    instrumentClass @17 :Enums.InstrumentClass;

    # Currencies
    baseCurrency @18 :Types.Currency;  # Optional
    quoteCurrency @19 :Types.Currency;
    settlementCurrency @20 :Types.Currency;

    # Timestamps
    tsInit @21 :Base.UnixNanos;
    tsOpened @22 :Base.UnixNanos;
    tsLast @23 :Base.UnixNanos;
    tsClosed @24 :Base.UnixNanos;  # Optional - 0 means None
    durationNs @25 :UInt64;

    # P&L and pricing
    avgPxOpen @26 :Float64;
    avgPxClose @27 :Float64;  # Optional
    realizedReturn @28 :Float64;
    realizedPnl @29 :Types.Money;  # Optional

    # Trade tracking
    tradeIds @30 :List(Identifiers.TradeId);
    buyQty @31 :Types.Quantity;
    sellQty @32 :Types.Quantity;

    # Commissions map represented as parallel lists
    commissionCurrencies @33 :List(Types.Currency);
    commissionAmounts @34 :List(Types.Money);
}

# Order list containing multiple orders
struct OrderList {
    orders @0 :List(Order);
    instrumentId @1 :Identifiers.InstrumentId;
    orderListId @2 :Identifiers.OrderListId;
}

# Trading command variants
struct TradingCommand {
    union {
        submitOrder @0 :SubmitOrder;
        submitOrderList @1 :SubmitOrderList;
        modifyOrder @2 :ModifyOrder;
        cancelOrder @3 :CancelOrder;
        cancelAllOrders @4 :CancelAllOrders;
        batchCancelOrders @5 :BatchCancelOrders;
        queryOrder @6 :QueryOrder;
        queryAccount @7 :QueryAccount;
    }
}

struct SubmitOrder {
    header @0 :TradingCommandHeader;
    orderInit @1 :OrderEvents.OrderInitialized;
    positionId @2 :Identifiers.PositionId;
}

struct SubmitOrderList {
    header @0 :TradingCommandHeader;
    orderInits @1 :List(OrderEvents.OrderInitialized);
    positionId @2 :Identifiers.PositionId;
}

struct ModifyOrder {
    header @0 :TradingCommandHeader;
    clientOrderId @1 :Identifiers.ClientOrderId;
    venueOrderId @2 :Identifiers.VenueOrderId;
    quantity @3 :Types.Quantity;
    price @4 :Types.Price;
    triggerPrice @5 :Types.Price;
}

struct CancelOrder {
    header @0 :TradingCommandHeader;
    clientOrderId @1 :Identifiers.ClientOrderId;
    venueOrderId @2 :Identifiers.VenueOrderId;
}

struct CancelAllOrders {
    header @0 :TradingCommandHeader;
    orderSide @1 :Enums.OrderSide;
}

struct BatchCancelOrders {
    header @0 :TradingCommandHeader;
    cancellations @1 :List(CancelOrder);
}

struct QueryOrder {
    header @0 :TradingCommandHeader;
    clientOrderId @1 :Identifiers.ClientOrderId;
    venueOrderId @2 :Identifiers.VenueOrderId;
}

struct QueryAccount {
    traderId @0 :Identifiers.TraderId;
    accountId @1 :Identifiers.AccountId;
    commandId @2 :Base.UUID4;
    tsInit @3 :Base.UnixNanos;
}
