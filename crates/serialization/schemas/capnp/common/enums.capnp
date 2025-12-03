@0xb2c3d4e5f6071829;
# Cap'n Proto schema for Nautilus enum types

enum AccountType {
    cash @0;
    margin @1;
    betting @2;
    wallet @3;
}

enum AggressorSide {
    noAggressor @0;
    buyer @1;
    seller @2;
}

enum AssetClass {
    fx @0;
    equity @1;
    commodity @2;
    debt @3;
    index @4;
    cryptocurrency @5;
    alternative @6;
}

enum InstrumentClass {
    spot @0;
    swap @1;
    future @2;
    futuresSpread @3;
    forward @4;
    cfd @5;  # Contract for Difference
    bond @6;
    option @7;
    optionSpread @8;
    warrant @9;
    sportsBetting @10;
    binaryOption @11;
}

enum OptionKind {
    call @0;
    put @1;
}

enum OrderSide {
    noOrderSide @0;
    buy @1;
    sell @2;
}

enum OrderType {
    market @0;
    limit @1;
    stopMarket @2;
    stopLimit @3;
    marketToLimit @4;
    marketIfTouched @5;
    limitIfTouched @6;
    trailingStopMarket @7;
    trailingStopLimit @8;
}

enum OrderStatus {
    initialized @0;
    denied @1;
    emulated @2;
    released @3;
    submitted @4;
    accepted @5;
    rejected @6;
    canceled @7;
    expired @8;
    triggered @9;
    pendingUpdate @10;
    pendingCancel @11;
    partiallyFilled @12;
    filled @13;
}

enum TimeInForce {
    gtc @0;  # Good Till Cancel
    ioc @1;  # Immediate Or Cancel
    fok @2;  # Fill Or Kill
    gtd @3;  # Good Till Date
    day @4;  # Day
    atTheOpen @5;
    atTheClose @6;
}

enum TriggerType {
    noTrigger @0;
    default @1;
    lastPrice @2;
    markPrice @3;
    indexPrice @4;
    bidAsk @5;
    doubleLast @6;
    doubleBidAsk @7;
    lastOrBidAsk @8;
    midPoint @9;
}

enum ContingencyType {
    noContingency @0;
    oco @1;  # One-Cancels-the-Other
    oto @2;  # One-Triggers-the-Other
    ouo @3;  # One-Updates-the-Other
}

enum PositionSide {
    noPositionSide @0;
    flat @1;
    long @2;
    short @3;
}

enum LiquiditySide {
    noLiquiditySide @0;
    maker @1;
    taker @2;
}

enum BookAction {
    add @0;
    update @1;
    delete @2;
    clear @3;
}

enum BookType {
    topOfBookBidOffer @0;  # Level 1 Top-of-book bid and offer
    marketByPrice @1;       # Level 2 Market by price
    marketByOrder @2;       # Level 3 Market by order
}

enum OrderBookDeltaType {
    add @0;
    update @1;
    delete @2;
    clear @3;
}

enum RecordFlag {
    fLast @0;       # Last message in book event (bit 7 = 128)
    fTob @1;        # Top-of-book message (bit 6 = 64)
    fSnapshot @2;   # Message from replay/snapshot (bit 5 = 32)
    fMbp @3;        # Market-by-price message (bit 4 = 16)
    reserved2 @4;   # Reserved for future use (bit 3 = 8)
    reserved1 @5;   # Reserved for future use (bit 2 = 4)
}

enum AggregationSource {
    external @0;
    internal @1;
}

enum PriceType {
    bid @0;
    ask @1;
    mid @2;
    last @3;
    mark @4;
}

enum BarAggregation {
    tick @0;
    tickImbalance @1;
    tickRuns @2;
    volume @3;
    volumeImbalance @4;
    volumeRuns @5;
    value @6;
    valueImbalance @7;
    valueRuns @8;
    millisecond @9;
    second @10;
    minute @11;
    hour @12;
    day @13;
    week @14;
    month @15;
    year @16;
    renko @17;
}

enum TrailingOffsetType {
    noTrailingOffset @0;
    price @1;
    basisPoints @2;
    ticks @3;
    priceTier @4;
}

enum OmsType {
    unspecified @0;
    netting @1;
    hedging @2;
}

enum CurrencyType {
    crypto @0;
    fiat @1;
    commodityBacked @2;
}

enum InstrumentCloseType {
    endOfSession @0;
    contractExpired @1;
}

enum MarketStatusAction {
    none @0;
    preOpen @1;
    preCross @2;
    quoting @3;
    cross @4;
    rotation @5;
    newPriceIndication @6;
    trading @7;
    halt @8;
    pause @9;
    suspend @10;
    preClose @11;
    close @12;
    postClose @13;
    shortSellRestrictionChange @14;
    notAvailableForTrading @15;
}

enum PositionAdjustmentType {
    commission @0;
    funding @1;
}
